//! Each form of a document: its bytes, the bundle read back from them, and
//! the text and files a store keeps for them.

use alloc::collections::BTreeMap;
use std::io::{Cursor, Read as _};
use std::path::Path;

use chrono::{DateTime, NaiveTime, Timelike as _, Utc};
use os_shim::System as _;
use os_shim::mock::MemorySystem;

use super::{DecodeError, EncodeError, Form, FormError, StoredBundle, decode, encode, stored};
use crate::bundle::{BundledDocument, BundledDocumentError, Missing};
use crate::parser::ast::{FrontmatterFormat, RawDocument, Value};
use crate::parser::parse;
use crate::parser::tests::FIXTURES;
use crate::parser::tests::json::unclaim;
use crate::writer::{WriteError, WriteOptions, write};

const FORMS: &[Form] = &[
    Form::Json,
    Form::Markdown,
    #[cfg(feature = "binary-format")]
    Form::Binary,
    Form::Zip,
];

const TOML: WriteOptions = WriteOptions {
    frontmatter: FrontmatterFormat::Toml,
};

/// The bytes of a test picture.
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n not a real picture";

/// The text of the sample bundle, which embeds `media/logo.png`.
const REPORT: &str = "# Report\n\n![Logo](media/logo.png)\n";

/// The binary form of `toml_multi_line_string.md`. A change to a model type
/// changes it, and raises `BINARY_VERSION` with it.
#[cfg(feature = "binary-format")]
const PINNED: &[u8] =
    b"arky\0\x04\x07\0\0\0Body.\xff\xff\xff\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\
    \xdc\xff\xff\xff\x01\0\0\0\0\0\0\0\x04\0\0\0\xf0\xff\xff\xff\x01\0\0\0\0\0\0\0\0\0\0\0\0\0\
    \0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\xd0\xff\xff\xff\x01\0\0\0\0\0\0\0\0\0\
    \0\0\0\0\0\0\xec\xff\xff\xff\0\0\0\0\0\0\0\0\0\0\0\0\x01\0\0\0h\xff4h\xff4\xff\xff\xff\xff\
    \xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xffShippi\
    ng the parserdescriptionThe parser reads a file\nin two stages.\0\0\0\0\0title\xff\xff\xff\
    \t\0\0\0\x93\0\0\0\xab\xff\xff\xff\0\0\0\0\0\0\0\0\0\0\0\0\x8b\0\0\0\xaa\xff\xff\xff\t\0\0\
    \0\xa6\0\0\0\xa9\xff\xff\xff\0\0\0\0\0\0\0\0\0\0\0\0\x01\0\0\0,\xff\xff\xff\x01\0\0\0\x01\
    \0\0\0H\xff\xff\xff\x02\0\0\0\x03\0\0\0\xa4\xff\xff\xff\x01\0\0\0\0\0\0\0";

/// `REPORT` and `PNG` at `media/logo.png`.
fn sample() -> BundledDocument {
    let files = BTreeMap::from([("media/logo.png".to_owned(), PNG.to_vec())]);
    BundledDocument::new(parse(REPORT).document, files, Missing::Refuse).unwrap()
}

/// `document` as a bundle of no file, each embed's target kept as written.
fn bundled(document: RawDocument) -> BundledDocument {
    BundledDocument::new(document, BTreeMap::new(), Missing::Keep).unwrap()
}

/// The text a store keeps for `document` put into `form`.
fn kept(document: &RawDocument, form: Form, options: &WriteOptions) -> Result<String, FormError> {
    let bytes = encode(&bundled(document.clone()), form, options)?;
    Ok(stored(&bytes, form, options, Missing::Keep)?.text)
}

/// `document` with each frontmatter value as the JSON form keeps it.
fn through_json(document: &RawDocument) -> RawDocument {
    let mut kept = document.clone();
    if let RawDocument::Valid(valid) = &mut kept
        && let Some(mapping) = &mut valid.frontmatter
    {
        for value in mapping.values_mut() {
            *value = value_through_json(value.clone());
        }
    }
    kept
}

/// What the JSON form keeps of `value`: a date-time in UTC, or a date when it
/// is at midnight UTC, and a time to the millisecond.
fn value_through_json(value: Value) -> Value {
    let in_utc = |at: DateTime<Utc>| {
        if at.time() == NaiveTime::MIN {
            Value::Date(at.date_naive())
        } else {
            Value::OffsetDateTime(at.fixed_offset())
        }
    };
    match value {
        Value::DateTime(at) => in_utc(at.and_utc()),
        Value::Mapping(inner) => Value::Mapping(
            inner
                .into_iter()
                .map(|(key, item)| (key, value_through_json(item)))
                .collect(),
        ),
        Value::OffsetDateTime(at) => in_utc(at.to_utc()),
        Value::Sequence(items) => {
            Value::Sequence(items.into_iter().map(value_through_json).collect())
        }
        Value::Time(time) => {
            let millisecond = time.nanosecond() / 1_000_000 * 1_000_000;
            Value::Time(time.with_nanosecond(millisecond).unwrap())
        }
        Value::Boolean(_)
        | Value::Date(_)
        | Value::Float(_)
        | Value::Integer(_)
        | Value::Null(())
        | Value::String(_) => value,
    }
}

/// The writer's error `form_error` holds, if it holds one.
fn write_error(form_error: FormError) -> Option<WriteError> {
    match form_error {
        FormError::Encode(
            EncodeError::Write(error)
            | EncodeError::BundledDocument(BundledDocumentError::Write(error)),
        ) => Some(error),
        #[cfg(feature = "binary-format")]
        FormError::Encode(EncodeError::Binary(_)) => None,
        FormError::Decode(_)
        | FormError::Encode(EncodeError::BundledDocument(_) | EncodeError::Json(_)) => None,
    }
}

#[test]
fn the_sample_puts_a_list_into_each_form() {
    let options = WriteOptions::default();
    let bundle = decode(b"* item\n", Form::Markdown, Missing::Refuse).unwrap();
    let json = encode(&bundle, Form::Json, &options).unwrap();
    assert!(json.ends_with(b"\n"));
    #[cfg(feature = "binary-format")]
    let binary = {
        let binary = encode(&bundle, Form::Binary, &options).unwrap();
        assert_eq!(&binary[..6], b"arky\0\x04");
        assert_eq!(
            stored(&binary, Form::Binary, &options, Missing::Refuse)
                .unwrap()
                .text,
            "- item\n"
        );
        binary
    };
    assert_eq!(
        stored(&json, Form::Json, &options, Missing::Refuse)
            .unwrap()
            .text,
        "- item\n"
    );
    #[cfg(feature = "binary-format")]
    {
        let mut wrong = binary;
        wrong[5] = 1;
        assert!(matches!(
            decode(&wrong, Form::Binary, Missing::Refuse),
            Err(DecodeError::Version { found: 1 })
        ));
    }
}

#[test]
fn each_form_of_the_sample_bundle_reads_back_as_the_same_bundle() {
    let bundle = sample();
    for form in FORMS.iter().copied() {
        let bytes = encode(&bundle, form, &WriteOptions::default()).unwrap();
        assert_eq!(
            decode(&bytes, form, Missing::Refuse).unwrap(),
            bundle,
            "{form:?}"
        );
    }
}

#[test]
fn a_bundle_whose_address_holds_a_backslash_reads_back_from_each_form_with_its_file() {
    let logo = |text: &str| {
        let files = BTreeMap::from([("media/logo.png".to_owned(), PNG.to_vec())]);
        BundledDocument::new(parse(text).document, files, Missing::Refuse).unwrap()
    };
    let bundle = logo("![x](media\\logo.png)\n");
    let named = logo("![x](media/logo.png)\n");
    for form in FORMS.iter().copied() {
        let bytes = encode(&bundle, form, &WriteOptions::default()).unwrap();
        // A data URI names its file by its path, and that name is the target read back.
        let expected = if matches!(form, Form::Json | Form::Markdown) {
            &named
        } else {
            &bundle
        };
        assert_eq!(
            &decode(&bytes, form, Missing::Refuse).unwrap(),
            expected,
            "{form:?}"
        );
    }
}

#[test]
fn markdown_and_json_hold_each_file_as_a_named_data_uri_and_the_zip_holds_it_at_its_path() {
    let options = WriteOptions::default();
    let uri = "data:image/png;name=media/logo.png;base64,iVBORw0KGgogbm90IGEgcmVhbCBwaWN0dXJl";
    let markdown = encode(&sample(), Form::Markdown, &options).unwrap();
    assert_eq!(
        String::from_utf8(markdown).unwrap(),
        format!("# Report\n\n![Logo]({uri})\n")
    );
    let json = String::from_utf8(encode(&sample(), Form::Json, &options).unwrap()).unwrap();
    assert!(json.contains(&format!("\"tg\":\"{uri}\"")), "{json}");
    let zip = encode(&sample(), Form::Zip, &options).unwrap();
    let system = MemorySystem::from_zip_reader(Cursor::new(zip)).unwrap();
    let mut listed = system.read_dir(Path::new("/")).unwrap();
    listed.extend(system.read_dir(Path::new("/media")).unwrap());
    listed.sort();
    assert_eq!(
        listed,
        ["/index.md", "/media", "/media/logo.png"].map(Path::new)
    );
    assert_eq!(
        system.read_to_string(Path::new("/index.md")).unwrap(),
        REPORT
    );
    let mut logo = Vec::new();
    system
        .open(Path::new("/media/logo.png"))
        .unwrap()
        .read_to_end(&mut logo)
        .unwrap();
    assert_eq!(logo, PNG);
}

#[cfg(feature = "binary-format")]
#[test]
fn the_binary_form_opens_with_version_4_and_holds_the_files() {
    let binary = encode(&sample(), Form::Binary, &WriteOptions::default()).unwrap();
    assert_eq!(&binary[..6], b"arky\0\x04");
    assert!(binary.windows(PNG.len()).any(|window| window == PNG));
}

#[test]
fn each_form_of_the_sample_bundle_is_stored_as_its_text_and_its_files() {
    let options = WriteOptions::default();
    let expected = StoredBundle {
        files: BTreeMap::from([("media/logo.png".to_owned(), PNG.to_vec())]),
        text: REPORT.to_owned(),
    };
    for form in FORMS.iter().copied() {
        let bytes = encode(&sample(), form, &options).unwrap();
        assert_eq!(
            stored(&bytes, form, &options, Missing::Refuse).unwrap(),
            expected,
            "{form:?}"
        );
    }
}

#[test]
fn an_embed_with_no_file_is_refused_unless_it_is_kept() {
    let options = WriteOptions::default();
    let refused = decode(REPORT.as_bytes(), Form::Markdown, Missing::Refuse).unwrap_err();
    assert_eq!(
        refused.to_string(),
        "index.md names media/logo.png, and the bundle does not hold it"
    );
    assert!(
        matches!(
            &refused,
            DecodeError::BundledDocument(BundledDocumentError::Missing { target })
                if target == "media/logo.png"
        ),
        "{refused:?}"
    );
    assert!(matches!(
        stored(REPORT.as_bytes(), Form::Markdown, &options, Missing::Refuse),
        Err(FormError::Decode(DecodeError::BundledDocument(
            BundledDocumentError::Missing { target: _ }
        )))
    ));
    let bundle = decode(REPORT.as_bytes(), Form::Markdown, Missing::Keep).unwrap();
    assert_eq!(bundle.files(), Vec::<&str>::new());
    assert_eq!(bundle.document(), &parse(REPORT).document);
    assert_eq!(
        stored(REPORT.as_bytes(), Form::Markdown, &options, Missing::Keep).unwrap(),
        StoredBundle {
            files: BTreeMap::new(),
            text: REPORT.to_owned(),
        }
    );
}

#[test]
fn a_zip_whose_text_does_not_write_fails_with_the_bundles_error() {
    let bundle = decode(b"---\na: null\n---\n# A\n", Form::Markdown, Missing::Refuse).unwrap();
    let refused = encode(&bundle, Form::Zip, &TOML).unwrap_err();
    assert!(
        matches!(
            refused,
            EncodeError::BundledDocument(BundledDocumentError::Write(_))
        ),
        "{refused:?}"
    );
}

#[test]
fn each_fixture_is_stored_from_each_form_as_write_writes_it_and_a_second_pass_changes_nothing() {
    for source in FIXTURES {
        let document = parse(source).document;
        let through = through_json(&document);
        for form in FORMS.iter().copied() {
            let expected = if form == Form::Json {
                &through
            } else {
                &document
            };
            for options in [WriteOptions::default(), TOML] {
                let text = kept(&document, form, &options).map_err(write_error);
                assert_eq!(
                    text,
                    write(expected, &options).map_err(Some),
                    "{form:?} {options:?}: {source}"
                );
                if let Ok(first) = text {
                    let again = kept(&parse(&first).document, form, &options);
                    assert_eq!(
                        again.map_err(write_error),
                        Ok(first),
                        "{form:?} {options:?}: {source}"
                    );
                }
            }
        }
    }
}

#[cfg(feature = "binary-format")]
#[test]
fn each_fixture_reads_back_from_the_binary_form_every_kind_empty() {
    for source in FIXTURES {
        let mut document = parse(source).document;
        let bundle = bundled(document.clone());
        let bytes = encode(&bundle, Form::Binary, &WriteOptions::default()).unwrap();
        unclaim(&mut document);
        assert_eq!(
            decode(&bytes, Form::Binary, Missing::Keep).unwrap(),
            bundled(document),
            "{source}"
        );
    }
}

#[test]
fn bytes_that_are_not_utf8_fail_naming_the_offset_of_the_first_byte_that_is_not() {
    for form in [Form::Json, Form::Markdown] {
        assert!(matches!(
            decode(b"a\xffb", form, Missing::Keep),
            Err(DecodeError::NotUtf8 { offset: 1 })
        ));
        assert!(matches!(
            stored(b"a\xffb", form, &WriteOptions::default(), Missing::Keep),
            Err(FormError::Decode(DecodeError::NotUtf8 { offset: 1 }))
        ));
    }
}

#[cfg(feature = "binary-format")]
#[test]
fn binary_bytes_that_do_not_open_with_arky_a_zero_byte_and_a_version_are_not_arkys() {
    for bytes in [b"bincode!".as_slice(), b"arky\0", b"arky", b"x", b""] {
        assert!(matches!(
            decode(bytes, Form::Binary, Missing::Keep),
            Err(DecodeError::NotArky)
        ));
        assert!(matches!(
            stored(bytes, Form::Binary, &WriteOptions::default(), Missing::Keep),
            Err(FormError::Decode(DecodeError::NotArky))
        ));
    }
}

#[cfg(feature = "binary-format")]
#[test]
fn binary_bytes_of_another_version_fail_naming_it() {
    let bundle = bundled(parse(FIXTURES[0]).document);
    let bytes = encode(&bundle, Form::Binary, &WriteOptions::default()).unwrap();
    for version in [0, 1, 2, 3, 5, u8::MAX] {
        let mut other = bytes.clone();
        other[5] = version;
        assert!(matches!(
            decode(&other, Form::Binary, Missing::Keep),
            Err(DecodeError::Version { found }) if found == version
        ));
        assert!(matches!(
            stored(&other, Form::Binary, &WriteOptions::default(), Missing::Keep),
            Err(FormError::Decode(DecodeError::Version { found })) if found == version
        ));
    }
}

#[cfg(feature = "binary-format")]
#[test]
fn binary_bytes_of_version_3_are_refused_naming_the_version_this_arky_reads() {
    let mut bytes = encode(&sample(), Form::Binary, &WriteOptions::default()).unwrap();
    bytes[5] = 3;
    assert_eq!(
        decode(&bytes, Form::Binary, Missing::Refuse)
            .unwrap_err()
            .to_string(),
        "arky's binary form version 3, and this arky reads version 4"
    );
}

#[test]
fn the_json_form_ends_with_one_newline_and_reads_with_or_without_it() {
    for source in FIXTURES {
        let mut document = parse(source).document;
        let json = encode(
            &bundled(document.clone()),
            Form::Json,
            &WriteOptions::default(),
        )
        .unwrap();
        let (&last, value) = json.split_last().unwrap();
        assert_eq!(last, b'\n', "{source}");
        assert!(!value.ends_with(b"\n"), "{source}");
        unclaim(&mut document);
        let expected = bundled(document);
        assert_eq!(
            decode(&json, Form::Json, Missing::Keep).unwrap(),
            expected,
            "{source}"
        );
        assert_eq!(
            decode(value, Form::Json, Missing::Keep).unwrap(),
            expected,
            "{source}"
        );
    }
}

#[test]
fn a_yaml_fixture_is_stored_with_toml_frontmatter_from_each_form_when_the_options_name_toml() {
    let source = include_str!("../parser/tests/fixtures/jekyll_post.md");
    assert!(source.starts_with("---\n"));
    let document = parse(source).document;
    for form in FORMS.iter().copied() {
        let bytes = encode(&bundled(document.clone()), form, &WriteOptions::default()).unwrap();
        let text = stored(&bytes, form, &TOML, Missing::Keep).unwrap().text;
        assert!(text.starts_with("+++\n"), "{form:?}: {text}");
        assert_eq!(text, write(&document, &TOML).unwrap(), "{form:?}");
    }
}

#[cfg(feature = "binary-format")]
#[test]
fn the_binary_form_of_the_smallest_fixture_is_pinned() {
    let source = include_str!("../parser/tests/fixtures/toml_multi_line_string.md");
    let bundle = bundled(parse(source).document);
    let bytes = encode(&bundle, Form::Binary, &WriteOptions::default()).unwrap();
    assert_eq!(bytes, PINNED);
}
