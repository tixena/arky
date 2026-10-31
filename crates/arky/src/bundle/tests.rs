//! A bundle made from a tree, a file or a zip: the files it carries, the path each target
//! names, the walks over its embeds, and its errors.

use alloc::collections::BTreeMap;
use std::io::Write as _;
use std::path::Path;

use base64::DecodeError;
use os_shim::System as _;
use os_shim::archive::{ZipOptions, ZipStreamWriter, to_zip_writer};
use os_shim::mock::MemorySystem;

use super::{
    BundledDocument, BundledDocumentError, Embed, Missing, data_of, embedded_paths, embeds,
    images_mut, path_of, read,
};
use crate::forms::{Form, decode, encode};
use crate::parser::ast::{LinkForm, RawDocument};
use crate::parser::lexer::BLOCK_CAP;
use crate::parser::parse;
use crate::writer::{WriteOptions, write};

/// The bytes of a test picture.
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n not a real picture";

/// A picture in a heading, a block quote, a quote inside a quote, a list item, a table cell, a
/// link's text and emphasis, a wiki embed, an HTML picture and a code block.
const PLACES: &str = "# Head ![h](media/h.png)

> quoted ![q](media/q.png)
>
> > nested ![n](media/n.png)

- item ![i](media/i.png)

| a | b |
|---|---|
| ![t](media/t.png) | x |

[![badge](media/b.png)](https://ci)

*em ![e](media/e.png)*

![[note#Heading|alt]]

<img src=\"media/html.png\">

```
![c](media/code.png)
```
";

/// The targets of `PLACES`, in document order.
const TARGETS: [&str; 8] = [
    "media/h.png",
    "media/q.png",
    "media/n.png",
    "media/i.png",
    "media/t.png",
    "media/b.png",
    "media/e.png",
    "note#Heading",
];

/// The embed of `form` with `target` and `alt`.
fn embed(form: LinkForm, target: &str, alt: &str) -> Embed {
    Embed {
        alt: alt.to_owned(),
        form,
        target: target.to_owned(),
    }
}

/// `pairs` as the files a bundle is made from.
fn files(pairs: &[(&str, &[u8])]) -> BTreeMap<String, Vec<u8>> {
    pairs
        .iter()
        .map(|(name, bytes)| ((*name).to_owned(), bytes.to_vec()))
        .collect()
}

/// The names the central directory of `zip` lists, in its order.
fn entries(zip: &[u8]) -> Vec<String> {
    let end = zip.len() - 22;
    assert_eq!(&zip[end..end + 4], b"PK\x05\x06");
    let count = number(&zip[end + 10..end + 12]);
    let mut at = number(&zip[end + 16..end + 20]);
    let mut names = Vec::new();
    for _ in 0..count {
        assert_eq!(&zip[at..at + 4], b"PK\x01\x02");
        let length = number(&zip[at + 28..at + 30]);
        let extra = number(&zip[at + 30..at + 32]);
        let comment = number(&zip[at + 32..at + 34]);
        names.push(String::from_utf8(zip[at + 46..at + 46 + length].to_vec()).unwrap());
        at += 46 + length + extra + comment;
    }
    names
}

/// The number `bytes` write, least significant byte first.
fn number(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .rev()
        .fold(0, |value, byte| (value << 8_u32) | usize::from(*byte))
}

/// The zip `system` holds at its root, as `to_zip_writer` writes it.
async fn zip_of(system: &MemorySystem) -> Vec<u8> {
    let mut zip = Vec::new();
    to_zip_writer(system, Path::new("/"), &mut zip, ZipOptions::default())
        .await
        .unwrap();
    zip
}

/// The text of `bundle` written embedded.
fn embedded_text(bundle: &BundledDocument) -> String {
    write(&bundle.embedded().unwrap(), &WriteOptions::default()).unwrap()
}

/// The bundle of `text` read with no file given.
fn read_back(text: &str) -> BundledDocument {
    BundledDocument::new(parse(text).document, BTreeMap::new(), Missing::Refuse).unwrap()
}

/// The text of the note whose data URI is the target of the first embed of `document`.
fn first_note(document: &RawDocument) -> String {
    let data = data_of(&embeds(document)[0].target).unwrap().unwrap();
    assert_eq!(data.media, "text/markdown");
    String::from_utf8(data.bytes).unwrap()
}

#[test]
fn the_sample_round_trips_through_a_pipe() {
    let document = parse("# Report\n\n![Logo](media/logo.png)\n").document;
    let given = files(&[("media/logo.png", PNG)]);
    let bundle = BundledDocument::new(document.clone(), given, Missing::Refuse).unwrap();
    let mut zip = Vec::new();
    bundle.to_zip(&mut zip, &WriteOptions::default()).unwrap();
    let back = BundledDocument::from_zip(&mut zip.as_slice(), Missing::Refuse).unwrap();
    assert_eq!(back.files(), ["media/logo.png"]);
    assert_eq!(back.file("media/logo.png"), Some(PNG));
    assert_eq!(back.document(), &document);
    assert_eq!(back, bundle);
}

#[test]
fn the_zip_of_the_sample_lists_its_folder_its_index_and_its_picture() {
    let document = parse("# Report\n\n![Logo](media/logo.png)\n").document;
    let given = files(&[("media/logo.png", PNG)]);
    let bundle = BundledDocument::new(document, given, Missing::Refuse).unwrap();
    let mut zip = Vec::new();
    bundle.to_zip(&mut zip, &WriteOptions::default()).unwrap();
    assert_eq!(entries(&zip), ["media/", "index.md", "media/logo.png"]);
}

#[test]
fn a_target_maps_to_its_file_path() {
    for (target, path) in [
        ("media/logo.png", "media/logo.png"),
        ("./media/logo.png", "media/logo.png"),
        ("media/../media/logo.png", "media/logo.png"),
        ("media/my%20logo.png", "media/my logo.png"),
        ("./media/logo.png?v=2#x", "media/logo.png"),
        ("dark.png#gh-dark-mode-only", "dark.png"),
        ("media/c%23.png", "media/c#.png"),
    ] {
        assert_eq!(path_of(target).unwrap().as_deref(), Some(path), "{target}");
    }
    for target in [
        "https://example.com/logo.png",
        "data:image/png;base64,iVBORw0KGgo=",
        "/srv/logo.png",
        "//cdn.example.com/logo.png",
        "C:/logo.png",
        "#top",
    ] {
        assert_eq!(path_of(target).unwrap(), None, "{target}");
    }
    for target in ["../logo.png", "media/../../logo.png", "%2E%2E/logo.png"] {
        assert!(
            matches!(
                path_of(target),
                Err(BundledDocumentError::Escapes { target: named }) if named == target
            ),
            "{target}"
        );
    }
    assert!(matches!(
        path_of("media/%FF.png"),
        Err(BundledDocumentError::Missing { target }) if target == "media/%FF.png"
    ));
}

#[test]
fn a_backslash_in_an_address_is_a_path_separator() {
    for target in ["media\\logo.png", "media%5Clogo.png", "media%5clogo.png"] {
        assert_eq!(
            path_of(target).unwrap().as_deref(),
            Some("media/logo.png"),
            "{target}"
        );
    }
    for target in [
        "\\\\server\\logo.png",
        "\\logo.png",
        "%5C%5Cserver/logo.png",
    ] {
        assert_eq!(path_of(target).unwrap(), None, "{target}");
    }
    let document = parse("![x](media\\logo.png)\n\n![y](\\server\\logo.png)\n").document;
    let given = files(&[("media/logo.png", PNG)]);
    let bundle = BundledDocument::new(document, given, Missing::Refuse).unwrap();
    assert_eq!(bundle.files(), ["media/logo.png"]);
    assert_eq!(bundle.file("media/logo.png"), Some(PNG));
}

#[test]
fn a_path_holding_a_backslash_is_a_file_no_bundle_can_hold() {
    let named = files(&[("x.png", PNG), ("a\\b.png", PNG)]);
    assert!(matches!(
        BundledDocument::new(parse("![x](x.png)\n").document, named, Missing::Keep),
        Err(BundledDocumentError::NotRelative { name }) if name == "a\\b.png"
    ));
    let document = parse("![[a\\b.png]]\n\n![y](media/logo.png)\n").document;
    let given = files(&[("media/logo.png", PNG)]);
    assert!(matches!(
        BundledDocument::new(document.clone(), given.clone(), Missing::Refuse),
        Err(BundledDocumentError::Missing { target }) if target == "a\\b.png"
    ));
    let kept = BundledDocument::new(document.clone(), given, Missing::Keep).unwrap();
    assert_eq!(kept.files(), ["media/logo.png"]);
    assert_eq!(kept.document(), &document);
    assert_eq!(
        embedded_paths(&document, "").unwrap(),
        [(
            embed(LinkForm::Markdown, "media/logo.png", "y"),
            "media/logo.png".to_owned()
        )]
    );
}

#[test]
fn a_wiki_target_and_a_note_s_target_are_paths_from_the_folder_of_the_text_that_holds_them() {
    let document = parse("![[logo.png|100]]\n\n![[sub/note#Heading]]\n").document;
    assert_eq!(
        embedded_paths(&document, "").unwrap(),
        [
            (
                embed(LinkForm::Wiki, "logo.png", "100"),
                "logo.png".to_owned()
            ),
            (
                embed(LinkForm::Wiki, "sub/note#Heading", ""),
                "sub/note.md".to_owned()
            ),
        ]
    );
    let note = parse("![chart](../media/chart.png)\n").document;
    assert_eq!(
        embedded_paths(&note, "parts").unwrap(),
        [(
            embed(LinkForm::Markdown, "../media/chart.png", "chart"),
            "media/chart.png".to_owned()
        )]
    );
}

#[test]
fn every_kind_of_place_holds_a_picture() {
    let found = embeds(&parse(PLACES).document);
    let targets: Vec<&str> = found.iter().map(|embed| embed.target.as_str()).collect();
    assert_eq!(targets, TARGETS);
    let forms: Vec<LinkForm> = found.iter().map(|embed| embed.form).collect();
    assert_eq!(forms[..7], [LinkForm::Markdown; 7]);
    assert_eq!(forms[7], LinkForm::Wiki);
}

#[test]
fn each_embed_carries_its_alt_text_or_its_wiki_alias_and_an_empty_one_when_it_has_none() {
    let document =
        parse("![Picture](logo)\n\n![[logo|Note]]\n\n![](a.png)\n\n![[b.png]]\n").document;
    assert_eq!(
        embeds(&document),
        [
            embed(LinkForm::Markdown, "logo", "Picture"),
            embed(LinkForm::Wiki, "logo", "Note"),
            embed(LinkForm::Markdown, "a.png", ""),
            embed(LinkForm::Wiki, "b.png", ""),
        ]
    );
}

#[test]
fn each_embedded_path_comes_with_the_embed_that_resolves_to_it_when_two_embeds_share_a_target() {
    let document = parse("![Picture](logo)\n\n![[logo|Note]]\n").document;
    assert_eq!(
        embedded_paths(&document, "").unwrap(),
        [
            (
                embed(LinkForm::Markdown, "logo", "Picture"),
                "logo".to_owned()
            ),
            (embed(LinkForm::Wiki, "logo", "Note"), "logo.md".to_owned()),
        ]
    );
}

#[test]
fn each_image_is_given_in_document_order_and_only_a_quote_whose_images_change_is_written_again() {
    let source = format!("{PLACES}\n> * star\n");
    let mut document = parse(&source).document;
    let mut seen = Vec::new();
    images_mut(&mut document, &mut |image| {
        seen.push(image.target.clone());
        if image.target == "media/n.png" {
            "media/m.png".clone_into(&mut image.target);
        }
    })
    .unwrap();
    assert_eq!(seen, TARGETS);
    let written = write(&document, &WriteOptions::default()).unwrap();
    assert!(
        written.contains("> > nested ![n](media/m.png)\n"),
        "{written}"
    );
    assert!(written.ends_with("\n> * star\n"), "{written}");
    let targets: Vec<String> = embeds(&document)
        .into_iter()
        .map(|embed| embed.target)
        .collect();
    assert_eq!(targets[2], "media/m.png");
}

#[test]
fn every_embed_travels_and_a_link_does_not() {
    let document = parse(
        "![[logo.png|100]]\n\n![[sub/note#Heading]]\n\n![d](dark.png#gh-dark-mode-only)\n\n\
         [spec](docs/spec.pdf)\n",
    )
    .document;
    let given = files(&[
        ("dark.png", b"dark".as_slice()),
        ("logo.png", PNG),
        (
            "sub/note.md",
            b"# Heading\n\n![p](pic.png)\n\n[other](other.md)\n".as_slice(),
        ),
        ("sub/pic.png", b"pic".as_slice()),
    ]);
    let bundle = BundledDocument::new(document, given, Missing::Refuse).unwrap();
    assert_eq!(
        bundle.files(),
        ["dark.png", "logo.png", "sub/note.md", "sub/pic.png"]
    );
    let text = embedded_text(&bundle);
    assert_eq!(
        text,
        "![[data:image/png;name=logo.png;base64,iVBORw0KGgogbm90IGEgcmVhbCBwaWN0dXJl|100]]\n\n\
         ![[data:text/markdown;name=sub/note.md;base64,IyBIZWFkaW5nCgohW3BdKGRhdGE6aW1hZ2UvcG5nO25hb\
         WU9cGljLnBuZztiYXNlNjQsY0dsaikKCltvdGhlcl0ob3RoZXIubWQpCg==#Heading]]\n\n\
         ![d](data:image/png;name=dark.png;base64,ZGFyaw==#gh-dark-mode-only)\n\n\
         [spec](docs/spec.pdf)\n"
    );
    let back = read_back(&text);
    assert_eq!(back.files(), bundle.files());
    for path in bundle.files() {
        assert_eq!(back.file(path), bundle.file(path), "{path}");
    }
    let targets: Vec<String> = embeds(back.document())
        .into_iter()
        .map(|embed| embed.target)
        .collect();
    assert_eq!(
        targets,
        [
            "logo.png",
            "sub/note.md#Heading",
            "dark.png#gh-dark-mode-only"
        ]
    );
}

#[test]
fn an_embedded_note_carries_its_own_embeds_inside_its_data_uri() {
    let document = parse("![[sub/note]]\n").document;
    let given = files(&[
        ("sub/note.md", b"![p](pic.png)\n".as_slice()),
        ("sub/pic.png", b"pic".as_slice()),
    ]);
    let bundle = BundledDocument::new(document, given, Missing::Refuse).unwrap();
    assert_eq!(
        first_note(&bundle.embedded().unwrap()),
        "![p](data:image/png;name=pic.png;base64,cGlj)\n"
    );
}

#[test]
fn a_note_that_embeds_itself_is_read_once() {
    let document = parse("![[self]]\n").document;
    let given = files(&[("self.md", b"![[self]]\n".as_slice())]);
    let bundle = BundledDocument::new(document, given, Missing::Refuse).unwrap();
    assert_eq!(bundle.files(), ["self.md"]);
    let text = embedded_text(&bundle);
    assert_eq!(
        text,
        "![[data:text/markdown;name=self.md;base64,IVtbc2VsZl1dCg==]]\n"
    );
    let back = read_back(&text);
    assert_eq!(back.files(), ["self.md"]);
    assert_eq!(back.file("self.md"), Some(b"![[self]]\n".as_slice()));
}

#[test]
fn two_notes_that_embed_each_other_are_written_once_each() {
    let document = parse("![[a]]\n").document;
    let given = files(&[
        ("a.md", b"![[b]]\n".as_slice()),
        ("b.md", b"![[a]]\n".as_slice()),
    ]);
    let bundle = BundledDocument::new(document, given, Missing::Refuse).unwrap();
    let embedded = bundle.embedded().unwrap();
    let note_a = first_note(&embedded);
    assert!(note_a.starts_with("![[data:text/markdown;name=b.md;base64,"));
    assert_eq!(first_note(&parse(&note_a).document), "![[a]]\n");
    let back = read_back(&write(&embedded, &WriteOptions::default()).unwrap());
    assert_eq!(back.files(), ["a.md", "b.md"]);
}

#[test]
fn markdown_for_a_stream_carries_each_file_and_reads_back() {
    let document = parse(
        "# Report\n\n![Logo](media/my%20logo.png)\n\n> quoted ![q](media/q.png)\n\n\
         | a |\n|---|\n| ![t](media/t.png) |\n\n[![badge](media/b.png)](https://ci)\n",
    )
    .document;
    let given = files(&[
        ("media/b.png", b"b".as_slice()),
        ("media/my logo.png", PNG),
        ("media/q.png", b"q".as_slice()),
        ("media/t.png", b"t".as_slice()),
    ]);
    let bundle = BundledDocument::new(document, given, Missing::Refuse).unwrap();
    let text = embedded_text(&bundle);
    assert_eq!(
        text,
        "# Report\n\n\
         ![Logo](data:image/png;name=media/my%20logo.png;base64,iVBORw0KGgogbm90IGEgcmVhbCBwaWN0dXJl)\n\n\
         > quoted ![q](data:image/png;name=media/q.png;base64,cQ==)\n\n\
         | a                                                 |\n\
         | ------------------------------------------------- |\n\
         | ![t](data:image/png;name=media/t.png;base64,dA==) |\n\n\
         [![badge](data:image/png;name=media/b.png;base64,Yg==)](https://ci)\n"
    );
    assert_eq!(read_back(&text), bundle);
}

#[test]
fn json_for_a_stream_carries_each_file_and_reads_back() {
    let document = parse("![Logo](media/logo.png)\n").document;
    let given = files(&[("media/logo.png", PNG)]);
    let bundle = BundledDocument::new(document, given, Missing::Refuse).unwrap();
    let json = encode(&bundle, Form::Json, &WriteOptions::default()).unwrap();
    assert_eq!(
        String::from_utf8(json.clone()).unwrap(),
        "{\"t\":\"valid\",\"bd\":{\"se\":[{\"bl\":[{\"t\":\"paragraph\",\"li\":[{\"ch\":[{\"t\":\
         \"image\",\"at\":\"Logo\",\"co\":\"neither\",\"fr\":\"markdown\",\"tg\":\"data:image/png;\
         name=media/logo.png;base64,iVBORw0KGgogbm90IGEgcmVhbCBwaWN0dXJl\"}]}]}],\"ss\":[]}]}}\n"
    );
    assert_eq!(decode(&json, Form::Json, Missing::Refuse).unwrap(), bundle);
}

#[tokio::test]
async fn a_data_uri_with_no_name_becomes_a_file() {
    let document = parse(
        "![Logo](data:image/png;base64,iVBORw0KGgo=)\n\n![Icon](data:image/svg+xml,%3Csvg%2F%3E)\n",
    )
    .document;
    let bundle = BundledDocument::new(document, BTreeMap::new(), Missing::Refuse).unwrap();
    assert_eq!(
        bundle.files(),
        ["media/4c4b6a3be1314ab8.png", "media/d4dc56669143034f.svg"]
    );
    assert_eq!(
        bundle.file("media/4c4b6a3be1314ab8.png"),
        Some(b"\x89PNG\r\n\x1a\n".as_slice())
    );
    assert_eq!(
        bundle.file("media/d4dc56669143034f.svg"),
        Some(b"<svg/>".as_slice())
    );
    let system = MemorySystem::new();
    let path = Path::new("/out/report.md");
    bundle
        .to_file(&system, path, &WriteOptions::default())
        .await
        .unwrap();
    assert_eq!(
        system.read_to_string(path).await.unwrap(),
        "![Logo](media/4c4b6a3be1314ab8.png)\n\n![Icon](media/d4dc56669143034f.svg)\n"
    );
}

#[tokio::test]
async fn a_markdown_file_with_data_uris_is_written_back_with_files() {
    let system = MemorySystem::new()
        .with_file(
            "/notes/report.md",
            b"![Logo](data:image/png;name=media/logo.png;base64,iVBORw0KGgo=)\n",
        )
        .unwrap();
    let path = Path::new("/notes/report.md");
    let bundle = BundledDocument::from_file(&system, path, Missing::Refuse)
        .await
        .unwrap();
    assert_eq!(bundle.files(), ["media/logo.png"]);
    let copy = Path::new("/out/report.md");
    bundle
        .to_file(&system, copy, &WriteOptions::default())
        .await
        .unwrap();
    assert_eq!(
        system.read_to_string(copy).await.unwrap(),
        "![Logo](media/logo.png)\n"
    );
    assert_eq!(
        read(&system, Path::new("/out/media/logo.png"))
            .await
            .unwrap(),
        b"\x89PNG\r\n\x1a\n"
    );
}

#[test]
fn a_name_that_is_not_a_path_inside_the_bundle_gives_the_hashed_name() {
    for name in ["../logo.png", "/logo.png", "index.md", "%FF.png"] {
        let source = format!("![Logo](data:image/png;name={name};base64,iVBORw0KGgo=)\n");
        let bundle = read_back(&source);
        assert_eq!(bundle.files(), ["media/4c4b6a3be1314ab8.png"], "{name}");
        assert_eq!(
            embeds(bundle.document())[0].target,
            "media/4c4b6a3be1314ab8.png",
            "{name}"
        );
    }
}

#[test]
fn a_name_whose_path_holds_other_bytes_gives_the_hashed_name() {
    let source = "![a](media/logo.png)\n\n\
                  ![b](data:image/png;name=media/logo.png;base64,iVBORw0KGgo=)\n";
    let given = files(&[("media/logo.png", PNG)]);
    let bundle = BundledDocument::new(parse(source).document, given, Missing::Refuse).unwrap();
    assert_eq!(
        bundle.files(),
        ["media/4c4b6a3be1314ab8.png", "media/logo.png"]
    );
    assert_eq!(bundle.file("media/logo.png"), Some(PNG));
    let given_same = files(&[("media/logo.png", b"\x89PNG\r\n\x1a\n".as_slice())]);
    let same = BundledDocument::new(parse(source).document, given_same, Missing::Refuse).unwrap();
    assert_eq!(same.files(), ["media/logo.png"]);
}

#[test]
fn a_data_uri_never_overwrites_a_file_at_its_hashed_path_that_holds_other_bytes() {
    let source = "![a](media/4c4b6a3be1314ab8.png)\n\n![b](data:image/png;base64,iVBORw0KGgo=)\n";
    let given = files(&[("media/4c4b6a3be1314ab8.png", b"other".as_slice())]);
    let bundle = BundledDocument::new(parse(source).document, given, Missing::Refuse).unwrap();
    assert_eq!(
        bundle.files(),
        ["media/4c4b6a3be1314ab8-1.png", "media/4c4b6a3be1314ab8.png"]
    );
    assert_eq!(
        bundle.file("media/4c4b6a3be1314ab8.png"),
        Some(b"other".as_slice())
    );
    assert_eq!(
        bundle.file("media/4c4b6a3be1314ab8-1.png"),
        Some(b"\x89PNG\r\n\x1a\n".as_slice())
    );
    assert_eq!(
        embeds(bundle.document())[1].target,
        "media/4c4b6a3be1314ab8-1.png"
    );
}

#[test]
fn a_data_uri_whose_hashed_path_and_first_numbered_path_hold_other_bytes_goes_at_the_second() {
    let source = "![a](media/4c4b6a3be1314ab8.png)\n\n![b](media/4c4b6a3be1314ab8-1.png)\n\n\
                  ![c](data:image/png;base64,iVBORw0KGgo=)\n";
    let given = files(&[
        ("media/4c4b6a3be1314ab8.png", b"other".as_slice()),
        ("media/4c4b6a3be1314ab8-1.png", b"another".as_slice()),
    ]);
    let bundle = BundledDocument::new(parse(source).document, given, Missing::Refuse).unwrap();
    assert_eq!(
        bundle.files(),
        [
            "media/4c4b6a3be1314ab8-1.png",
            "media/4c4b6a3be1314ab8-2.png",
            "media/4c4b6a3be1314ab8.png"
        ]
    );
    assert_eq!(
        bundle.file("media/4c4b6a3be1314ab8-1.png"),
        Some(b"another".as_slice())
    );
    assert_eq!(
        bundle.file("media/4c4b6a3be1314ab8-2.png"),
        Some(b"\x89PNG\r\n\x1a\n".as_slice())
    );
    assert_eq!(
        embeds(bundle.document())[2].target,
        "media/4c4b6a3be1314ab8-2.png"
    );
}

#[test]
fn a_data_uri_shares_its_hashed_path_with_a_file_that_holds_the_same_bytes() {
    let source = "![a](media/4c4b6a3be1314ab8.png)\n\n![b](data:image/png;base64,iVBORw0KGgo=)\n";
    let given = files(&[(
        "media/4c4b6a3be1314ab8.png",
        b"\x89PNG\r\n\x1a\n".as_slice(),
    )]);
    let bundle = BundledDocument::new(parse(source).document, given, Missing::Refuse).unwrap();
    assert_eq!(bundle.files(), ["media/4c4b6a3be1314ab8.png"]);
    assert_eq!(
        embeds(bundle.document())[1].target,
        "media/4c4b6a3be1314ab8.png"
    );
}

#[test]
fn a_data_uri_given_a_numbered_path_reads_back_from_the_embedded_text_with_the_same_files() {
    let source = "![a](media/4c4b6a3be1314ab8.png)\n\n![b](data:image/png;base64,iVBORw0KGgo=)\n";
    let given = files(&[("media/4c4b6a3be1314ab8.png", b"other".as_slice())]);
    let bundle = BundledDocument::new(parse(source).document, given, Missing::Refuse).unwrap();
    let back = read_back(&embedded_text(&bundle));
    assert_eq!(back.files(), bundle.files());
    for file in bundle.files() {
        assert_eq!(back.file(file), bundle.file(file), "{file}");
    }
}

#[test]
fn a_data_uri_whose_base64_does_not_decode_fails_even_when_missing_files_are_kept() {
    let target = "data:image/png;base64,iVBOR!";
    for missing in [Missing::Refuse, Missing::Keep] {
        let document = parse(&format!("![x]({target})\n")).document;
        let error = BundledDocument::new(document, BTreeMap::new(), missing).unwrap_err();
        assert_eq!(
            error.to_string(),
            "index.md names data:image/png;base64,iVBOR!, whose base64 does not decode"
        );
        assert!(
            matches!(
                error,
                BundledDocumentError::NotBase64 {
                    target: named,
                    source: DecodeError::InvalidByte(5, b'!'),
                } if named == target
            ),
            "{missing:?}"
        );
    }
}

#[test]
fn a_data_uri_with_no_comma_fails_even_when_missing_files_are_kept() {
    let target = "data:image/png;base64";
    for missing in [Missing::Refuse, Missing::Keep] {
        let document = parse(&format!("![x]({target})\n")).document;
        let error = BundledDocument::new(document, BTreeMap::new(), missing).unwrap_err();
        assert_eq!(
            error.to_string(),
            "index.md names data:image/png;base64, a data URI with no comma before its data"
        );
        assert!(
            matches!(
                error,
                BundledDocumentError::NotDataUri { target: named } if named == target
            ),
            "{missing:?}"
        );
    }
}

#[test]
fn a_data_uri_s_scheme_and_base64_marker_are_read_in_any_case() {
    for target in [
        "DATA:image/png;base64,iVBORw0KGgo=",
        "Data:image/png;BASE64,iVBORw0KGgo=",
    ] {
        let bundle = read_back(&format!("![x]({target})\n"));
        assert_eq!(bundle.files(), ["media/4c4b6a3be1314ab8.png"], "{target}");
        assert_eq!(
            bundle.file("media/4c4b6a3be1314ab8.png"),
            Some(b"\x89PNG\r\n\x1a\n".as_slice()),
            "{target}"
        );
        assert_eq!(
            embeds(bundle.document())[0].target,
            "media/4c4b6a3be1314ab8.png",
            "{target}"
        );
    }
}

#[test]
fn a_data_uri_whose_scheme_is_not_lower_case_with_no_comma_fails() {
    let target = "DATA:image/png;base64";
    let document = parse(&format!("![x]({target})\n")).document;
    let error = BundledDocument::new(document, BTreeMap::new(), Missing::Refuse).unwrap_err();
    assert!(matches!(
        error,
        BundledDocumentError::NotDataUri { target: named } if named == target
    ));
}

#[test]
fn a_data_uri_s_name_parameter_is_read_in_any_case() {
    let bundle = read_back("![x](data:image/png;NAME=media/logo.png;base64,iVBORw0KGgo=)\n");
    assert_eq!(bundle.files(), ["media/logo.png"]);
    assert_eq!(embeds(bundle.document())[0].target, "media/logo.png");
}

#[tokio::test]
async fn a_note_read_from_a_file_has_its_data_uris_made_files() {
    let system = MemorySystem::new()
        .with_file("/notes/report.md", b"![[note]]\n")
        .unwrap()
        .with_file(
            "/notes/note.md",
            b"![p](data:image/png;name=pic.png;base64,cGlj)\n",
        )
        .unwrap();
    let path = Path::new("/notes/report.md");
    let bundle = BundledDocument::from_file(&system, path, Missing::Refuse)
        .await
        .unwrap();
    assert_eq!(bundle.files(), ["note.md", "pic.png"]);
    assert_eq!(bundle.file("note.md"), Some(b"![p](pic.png)\n".as_slice()));
    assert_eq!(bundle.file("pic.png"), Some(b"pic".as_slice()));
    let copy = Path::new("/out/report.md");
    bundle
        .to_file(&system, copy, &WriteOptions::default())
        .await
        .unwrap();
    assert_eq!(system.read_to_string(copy).await.unwrap(), "![[note]]\n");
    assert_eq!(
        system
            .read_to_string(Path::new("/out/note.md"))
            .await
            .unwrap(),
        "![p](pic.png)\n"
    );
}

#[tokio::test]
async fn a_note_s_picture_above_its_folder_reads_back_from_the_embedded_text_at_its_path() {
    let system = MemorySystem::new()
        .with_file(
            "/notes/report.md",
            b"# Report\n\n![Logo](media/logo.png)\n\n![[parts/intro]]\n",
        )
        .unwrap()
        .with_file("/notes/media/logo.png", PNG)
        .unwrap()
        .with_file("/notes/parts/intro.md", b"![chart](../media/chart.png)\n")
        .unwrap()
        .with_file("/notes/media/chart.png", b"chart")
        .unwrap();
    let path = Path::new("/notes/report.md");
    let bundle = BundledDocument::from_file(&system, path, Missing::Refuse)
        .await
        .unwrap();
    let back = read_back(&embedded_text(&bundle));
    assert_eq!(
        back.files(),
        ["media/chart.png", "media/logo.png", "parts/intro.md"]
    );
    for file in bundle.files() {
        assert_eq!(back.file(file), bundle.file(file), "{file}");
    }
    assert_eq!(
        back.document(),
        &parse("# Report\n\n![Logo](media/logo.png)\n\n![[parts/intro.md]]\n").document
    );
}

#[test]
fn a_wiki_name_that_a_wiki_target_cannot_carry_gives_the_hashed_name() {
    for name in ["c%23.png", "c%7C.png", "c%5B.png", "c%5D.png"] {
        let bundle = read_back(&format!(
            "![[data:image/png;name={name};base64,iVBORw0KGgo=]]\n"
        ));
        assert_eq!(bundle.files(), ["media/4c4b6a3be1314ab8.png"], "{name}");
        assert_eq!(
            embeds(bundle.document())[0].target,
            "media/4c4b6a3be1314ab8.png",
            "{name}"
        );
        assert_eq!(read_back(&embedded_text(&bundle)), bundle, "{name}");
    }
}

#[test]
fn a_wiki_name_read_back_as_a_web_address_or_a_note_gives_the_hashed_name_and_keeps_its_bytes() {
    for name in ["c:d.png", "sub/a"] {
        let bundle = read_back(&format!(
            "![[data:image/png;name={name};base64,iVBORw0KGgo=]]\n"
        ));
        assert_eq!(bundle.files(), ["media/4c4b6a3be1314ab8.png"], "{name}");
        assert_eq!(
            bundle.file("media/4c4b6a3be1314ab8.png"),
            Some(b"\x89PNG\r\n\x1a\n".as_slice()),
            "{name}"
        );
        assert_eq!(
            embeds(bundle.document())[0].target,
            "media/4c4b6a3be1314ab8.png",
            "{name}"
        );
    }
}

#[test]
fn a_target_with_an_empty_part_reads_back_from_the_embedded_text_at_the_path_it_names() {
    let document = parse("![x](media//logo.png)\n").document;
    let given = files(&[("media/logo.png", PNG)]);
    let bundle = BundledDocument::new(document, given, Missing::Refuse).unwrap();
    let text = embedded_text(&bundle);
    assert_eq!(
        text,
        "![x](data:image/png;name=media//logo.png;base64,iVBORw0KGgogbm90IGEgcmVhbCBwaWN0dXJl)\n"
    );
    let back = read_back(&text);
    assert_eq!(back.files(), ["media/logo.png"]);
    assert_eq!(back.file("media/logo.png"), Some(PNG));
    assert_eq!(embeds(back.document())[0].target, "media//logo.png");
    assert_eq!(back, bundle);
}

#[tokio::test]
async fn a_note_on_disk_travels_with_its_pictures() {
    let system = MemorySystem::new()
        .with_file("/notes/report.md", b"# Report\n\n![[parts/intro]]\n")
        .unwrap()
        .with_file("/notes/parts/intro.md", b"![chart](../media/chart.png)\n")
        .unwrap()
        .with_file("/notes/media/chart.png", PNG)
        .unwrap();
    let path = Path::new("/notes/report.md");
    let bundle = BundledDocument::from_file(&system, path, Missing::Refuse)
        .await
        .unwrap();
    assert_eq!(bundle.files(), ["media/chart.png", "parts/intro.md"]);
    assert_eq!(bundle.file("media/chart.png"), Some(PNG));
    let mut zip = Vec::new();
    bundle.to_zip(&mut zip, &WriteOptions::default()).unwrap();
    let back = BundledDocument::from_zip(&mut zip.as_slice(), Missing::Refuse).unwrap();
    assert_eq!(back, bundle);
}

#[test]
fn a_named_file_that_is_not_given_fails_naming_its_target() {
    let document = parse("![Logo](media/my%20logo.png)\n").document;
    let error =
        BundledDocument::new(document.clone(), BTreeMap::new(), Missing::Refuse).unwrap_err();
    assert_eq!(
        error.to_string(),
        "index.md names media/my%20logo.png, and the bundle does not hold it"
    );
    let given = files(&[("media/my logo.png", PNG)]);
    let bundle = BundledDocument::new(document, given, Missing::Refuse).unwrap();
    assert_eq!(bundle.files(), ["media/my logo.png"]);
}

#[test]
fn a_target_above_the_folder_fails() {
    let document = parse("![Logo](../logo.png)\n").document;
    let error = BundledDocument::new(document, BTreeMap::new(), Missing::Refuse).unwrap_err();
    assert_eq!(
        error.to_string(),
        "index.md names ../logo.png, which leaves the bundle's folder"
    );
    assert!(matches!(error, BundledDocumentError::Escapes { target } if target == "../logo.png"));
}

#[test]
fn a_target_above_the_folder_is_refused_unless_it_is_allowed() {
    let document = parse("![Logo](../shared/logo.png)\n\n![y](media/logo.png)\n").document;
    let given = files(&[("media/logo.png", PNG)]);
    assert!(matches!(
        BundledDocument::new(document.clone(), given.clone(), Missing::Refuse),
        Err(BundledDocumentError::Escapes { target }) if target == "../shared/logo.png"
    ));
    let kept = BundledDocument::new(document.clone(), given, Missing::Keep).unwrap();
    assert_eq!(kept.files(), ["media/logo.png"]);
    assert_eq!(kept.document(), &document);
    assert_eq!(
        embedded_text(&kept),
        "![Logo](../shared/logo.png)\n\n\
         ![y](data:image/png;name=media/logo.png;base64,iVBORw0KGgogbm90IGEgcmVhbCBwaWN0dXJl)\n"
    );
}

#[test]
fn a_target_above_the_folder_has_no_embedded_path() {
    let document = parse("![Logo](../shared/logo.png)\n\n![y](media/logo.png)\n").document;
    assert_eq!(
        embedded_paths(&document, "").unwrap(),
        [(
            embed(LinkForm::Markdown, "media/logo.png", "y"),
            "media/logo.png".to_owned()
        )]
    );
}

#[tokio::test]
async fn a_note_s_target_above_the_folder_is_refused_unless_it_is_allowed() {
    let system = MemorySystem::new()
        .with_file("/notes/report.md", b"![[parts/intro]]\n")
        .unwrap()
        .with_file(
            "/notes/parts/intro.md",
            b"![x](../../shared/logo.png)\n\n![y](../media/logo.png)\n",
        )
        .unwrap()
        .with_file("/notes/media/logo.png", PNG)
        .unwrap();
    let path = Path::new("/notes/report.md");
    assert!(matches!(
        BundledDocument::from_file(&system, path, Missing::Refuse).await,
        Err(BundledDocumentError::Escapes { target }) if target == "../../shared/logo.png"
    ));
    let kept = BundledDocument::from_file(&system, path, Missing::Keep)
        .await
        .unwrap();
    assert_eq!(kept.files(), ["media/logo.png", "parts/intro.md"]);
    assert_eq!(
        first_note(&kept.embedded().unwrap()),
        "![x](../../shared/logo.png)\n\n\
         ![y](data:image/png;name=../media/logo.png;base64,iVBORw0KGgogbm90IGEgcmVhbCBwaWN0dXJl)\n"
    );
}

#[test]
fn a_file_the_document_does_not_name_is_left_out() {
    let document = parse("![Logo](media/logo.png)\n").document;
    let given = files(&[
        ("media/logo.png", PNG),
        ("media/unused.png", b"unused".as_slice()),
    ]);
    let bundle = BundledDocument::new(document, given, Missing::Refuse).unwrap();
    assert_eq!(bundle.files(), ["media/logo.png"]);
    assert_eq!(bundle.file("media/unused.png"), None);
}

#[test]
fn a_name_outside_the_bundle_is_refused() {
    for name in ["../x.png", "/x.png", "./x.png", "a//x.png", "index.md"] {
        let document = parse("![x](x.png)\n").document;
        let result = BundledDocument::new(document, files(&[(name, PNG)]), Missing::Keep);
        assert!(
            matches!(
                result,
                Err(BundledDocumentError::NotRelative { name: refused }) if refused == name
            ),
            "{name}"
        );
    }
}

#[tokio::test]
async fn a_file_and_its_pictures_make_a_bundle_and_back() {
    let system = MemorySystem::new()
        .with_file("/notes/report.md", b"# Report\n\n![Logo](media/logo.png)\n")
        .unwrap()
        .with_file("/notes/media/logo.png", PNG)
        .unwrap()
        .with_file("/notes/media/other.png", b"other")
        .unwrap();
    let path = Path::new("/notes/report.md");
    let bundle = BundledDocument::from_file(&system, path, Missing::Refuse)
        .await
        .unwrap();
    assert_eq!(bundle.files(), ["media/logo.png"]);
    let copy = Path::new("/out/copy.md");
    bundle
        .to_file(&system, copy, &WriteOptions::default())
        .await
        .unwrap();
    assert_eq!(
        system.read_to_string(copy).await.unwrap(),
        "# Report\n\n![Logo](media/logo.png)\n"
    );
    assert_eq!(
        read(&system, Path::new("/out/media/logo.png"))
            .await
            .unwrap(),
        PNG
    );
    assert!(
        !system
            .exists(Path::new("/out/media/other.png"))
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn a_file_whose_picture_is_absent_fails() {
    let system = MemorySystem::new()
        .with_file("/notes/report.md", b"![Logo](media/logo.png)\n")
        .unwrap();
    let path = Path::new("/notes/report.md");
    assert!(matches!(
        BundledDocument::from_file(&system, path, Missing::Refuse).await,
        Err(BundledDocumentError::Missing { target }) if target == "media/logo.png"
    ));
}

#[tokio::test]
async fn a_missing_file_is_refused_unless_it_is_allowed() {
    let document = parse("![Logo](media/logo.png)\n").document;
    assert!(matches!(
        BundledDocument::new(document.clone(), BTreeMap::new(), Missing::Refuse),
        Err(BundledDocumentError::Missing { target }) if target == "media/logo.png"
    ));
    let kept = BundledDocument::new(document.clone(), BTreeMap::new(), Missing::Keep).unwrap();
    assert_eq!(kept.files(), Vec::<&str>::new());
    assert_eq!(kept.document(), &document);
    let system = MemorySystem::new()
        .with_file("/notes/a.md", b"![Logo](media/logo.png)\n")
        .unwrap();
    let from_disk = BundledDocument::from_file(&system, Path::new("/notes/a.md"), Missing::Keep)
        .await
        .unwrap();
    from_disk
        .to_file(&system, Path::new("/out/a.md"), &WriteOptions::default())
        .await
        .unwrap();
    assert_eq!(
        system.read_to_string(Path::new("/out/a.md")).await.unwrap(),
        "![Logo](media/logo.png)\n"
    );
    assert_eq!(embedded_text(&from_disk), "![Logo](media/logo.png)\n");
}

#[test]
fn a_target_that_is_not_utf8_once_decoded_is_refused_unless_it_is_allowed() {
    let document = parse("![x](media/%FF.png)\n\n![y](media/logo.png)\n").document;
    let given = files(&[("media/logo.png", PNG)]);
    assert!(matches!(
        BundledDocument::new(document.clone(), given.clone(), Missing::Refuse),
        Err(BundledDocumentError::Missing { target }) if target == "media/%FF.png"
    ));
    let kept = BundledDocument::new(document.clone(), given, Missing::Keep).unwrap();
    assert_eq!(kept.files(), ["media/logo.png"]);
    assert_eq!(kept.document(), &document);
}

#[test]
fn a_target_that_is_not_utf8_once_decoded_has_no_embedded_path() {
    let document = parse("![x](media/%FF.png)\n\n![y](media/logo.png)\n").document;
    assert_eq!(
        embedded_paths(&document, "").unwrap(),
        [(
            embed(LinkForm::Markdown, "media/logo.png", "y"),
            "media/logo.png".to_owned()
        )]
    );
}

#[tokio::test]
async fn a_note_s_target_that_is_not_utf8_once_decoded_is_refused_unless_it_is_allowed() {
    let system = MemorySystem::new()
        .with_file("/notes/report.md", b"![[parts/intro]]\n")
        .unwrap()
        .with_file(
            "/notes/parts/intro.md",
            b"![x](../media/%FF.png)\n\n![y](../media/logo.png)\n",
        )
        .unwrap()
        .with_file("/notes/media/logo.png", PNG)
        .unwrap();
    let path = Path::new("/notes/report.md");
    assert!(matches!(
        BundledDocument::from_file(&system, path, Missing::Refuse).await,
        Err(BundledDocumentError::Missing { target }) if target == "../media/%FF.png"
    ));
    let kept = BundledDocument::from_file(&system, path, Missing::Keep)
        .await
        .unwrap();
    assert_eq!(kept.files(), ["media/logo.png", "parts/intro.md"]);
}

#[tokio::test]
async fn a_zip_with_every_entry_under_one_folder_reads() {
    let system = MemorySystem::new()
        .with_file("/report/index.md", b"![Logo](media/logo.png)\n")
        .unwrap()
        .with_file("/report/media/logo.png", PNG)
        .unwrap();
    let zip = zip_of(&system).await;
    assert!(entries(&zip).iter().all(|name| name.starts_with("report/")));
    let bundle = BundledDocument::from_zip(&mut zip.as_slice(), Missing::Refuse).unwrap();
    assert_eq!(bundle.files(), ["media/logo.png"]);
    assert_eq!(bundle.file("media/logo.png"), Some(PNG));
}

#[test]
fn a_zip_whose_sizes_follow_the_data_reads() {
    let mut writer = ZipStreamWriter::new(Vec::new());
    writer.start_entry("index.md").unwrap();
    writer.write_all(b"![Logo](media/logo.png)\n").unwrap();
    writer.start_entry("media/logo.png").unwrap();
    writer.write_all(PNG).unwrap();
    let zip = writer.finish().unwrap();
    assert!(MemorySystem::from_zip_stream(&mut zip.as_slice()).is_err());
    let bundle = BundledDocument::from_zip(&mut zip.as_slice(), Missing::Refuse).unwrap();
    assert_eq!(bundle.files(), ["media/logo.png"]);
    assert_eq!(bundle.file("media/logo.png"), Some(PNG));
}

#[tokio::test]
async fn a_zip_with_no_index_fails() {
    let system = MemorySystem::new()
        .with_file("/media/logo.png", PNG)
        .unwrap();
    let zip = zip_of(&system).await;
    assert!(matches!(
        BundledDocument::from_zip(&mut zip.as_slice(), Missing::Refuse),
        Err(BundledDocumentError::NoIndex)
    ));
}

#[test]
fn quotes_nested_deeper_than_the_block_cap_run_through_both_walks() {
    let marks = "> ".repeat(BLOCK_CAP + 60);
    let mut document = parse(&format!("{marks}![q](media/q.png)\n")).document;
    assert_eq!(
        embeds(&document),
        [embed(LinkForm::Markdown, "media/q.png", "q")]
    );
    images_mut(&mut document, &mut |image| {
        "media/r.png".clone_into(&mut image.target);
    })
    .unwrap();
    assert_eq!(
        write(&document, &WriteOptions::default()).unwrap(),
        format!("{marks}![q](media/r.png)\n")
    );
}
