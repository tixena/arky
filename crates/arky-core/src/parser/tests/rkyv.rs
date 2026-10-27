//! The tree written with rkyv, its bytes checked and read in place, and read
//! back. No node writes its `kind`, as in JSON.

use chrono::{FixedOffset, NaiveDate, NaiveTime, TimeZone as _};
use indexmap::IndexMap;
use rkyv::de::Pool;
use rkyv::rancor::{Error, Strategy};
use rkyv::util::AlignedVec;
use rkyv::with::{DeserializeWith as _, With};

use super::json::unclaim;
use super::{on_two_megabytes, valid};
use crate::parser::ast::{
    ArchivedRawDocument, ArchivedValue, Block, Document, Kind, Mapping, MarkdownBlock, RawDocument,
    Value,
};
use crate::parser::frontmatter::VALUE_CAP;
use crate::parser::lexer::BLOCK_CAP;
use crate::parser::mirror::RawDocumentMirror;
use crate::parser::parse;

pub const FIXTURES: [&str; 21] = [
    include_str!("fixtures/api_reference.md"),
    include_str!("fixtures/book_page.md"),
    include_str!("fixtures/changelog.md"),
    include_str!("fixtures/contact.md"),
    include_str!("fixtures/daily_note.md"),
    include_str!("fixtures/docusaurus_page.md"),
    include_str!("fixtures/essay.md"),
    include_str!("fixtures/hugo_post.md"),
    include_str!("fixtures/hugo_post_with_table.md"),
    include_str!("fixtures/issue_template.md"),
    include_str!("fixtures/jekyll_post.md"),
    include_str!("fixtures/meeting_notes.md"),
    include_str!("fixtures/obsidian_note.md"),
    include_str!("fixtures/pandoc_paper.md"),
    include_str!("fixtures/readme.md"),
    include_str!("fixtures/recipe.md"),
    include_str!("fixtures/slash_command.md"),
    include_str!("fixtures/task.md"),
    include_str!("fixtures/toml_multi_line_string.md"),
    include_str!("fixtures/yaml_list_of_mappings.md"),
    include_str!("fixtures/zola_page.md"),
];

/// The rkyv bytes of `document`.
fn to_bytes(document: &RawDocument) -> AlignedVec {
    rkyv::to_bytes::<Error>(With::<RawDocument, RawDocumentMirror>::cast(document)).unwrap()
}

/// The document `archived` holds.
fn deserialize(archived: &ArchivedRawDocument) -> RawDocument {
    RawDocumentMirror::deserialize_with(archived, Strategy::<_, Error>::wrap(&mut Pool::new()))
        .unwrap()
}

/// Writes `document` with rkyv, checks the bytes and reads them back: the
/// same tree, every `kind` empty.
fn reads_back_from_rkyv(mut document: RawDocument) {
    let bytes = to_bytes(&document);
    let archived = rkyv::access::<ArchivedRawDocument, Error>(&bytes).unwrap();
    let back = deserialize(archived);
    unclaim(&mut document);
    assert_eq!(back, document);
}

/// A valid document with an empty body and `mapping` as its frontmatter.
fn with_frontmatter(mapping: Mapping) -> RawDocument {
    RawDocument::Valid(Document {
        body: MarkdownBlock {
            kind: Kind(None),
            sections: Vec::new(),
        },
        frontmatter: Some(mapping),
        kind: Kind(None),
    })
}

/// The value under `key` in the frontmatter of `document`.
fn frontmatter_value(document: &RawDocument, key: &str) -> Value {
    let mapping = match document {
        RawDocument::Invalid(_) => None,
        RawDocument::Valid(valid) => valid.frontmatter.as_ref(),
    };
    mapping.unwrap()[key].clone()
}

/// How many mappings nest under the first key of `mapping`, each under the
/// first key of the one before.
fn mapping_depth(mapping: &Mapping) -> usize {
    let mut depth = 0;
    let mut inner = mapping;
    while let Some(Value::Mapping(next)) = inner.values().next() {
        depth += 1;
        inner = next;
    }
    depth
}

/// How many lists nest at the start of `body`, each in the first item of the
/// one before.
fn list_depth(body: &MarkdownBlock) -> usize {
    let mut depth = 0;
    let mut block = body;
    while let Some(Block::List(list)) = block
        .sections
        .first()
        .and_then(|section| section.blocks.first())
    {
        depth += 1;
        block = &list.items[0].body;
    }
    depth
}

#[test]
fn each_fixture_reads_back_from_rkyv() {
    for source in FIXTURES {
        reads_back_from_rkyv(parse(source).document);
    }
}

#[test]
fn an_invalid_document_reads_back_from_rkyv() {
    let document = parse("---\n{a: 1, b: [}\n---\n# A\n").document;
    assert!(matches!(document, RawDocument::Invalid(_)));
    reads_back_from_rkyv(document);
}

#[test]
fn the_sample_document_reads_in_place() {
    let parsed = parse("---\ndue: 2026-10-20\nat: 2026-10-20T09:00:00-04:00\n---\n# Task\n");
    let bytes = to_bytes(&parsed.document);
    let archived = rkyv::access::<ArchivedRawDocument, Error>(&bytes).unwrap();
    let document = match archived {
        ArchivedRawDocument::Invalid(_) => None,
        ArchivedRawDocument::Valid(document) => Some(document),
    };
    let frontmatter = document.unwrap().frontmatter.as_ref().unwrap();
    assert!(frontmatter.contains_key("due"));
    let offset = match frontmatter.get("at").unwrap() {
        ArchivedValue::OffsetDateTime(local_and_offset) => Some(local_and_offset.1.to_native()),
        ArchivedValue::Boolean(_)
        | ArchivedValue::Date(_)
        | ArchivedValue::DateTime(_)
        | ArchivedValue::Float(_)
        | ArchivedValue::Integer(_)
        | ArchivedValue::Mapping(_)
        | ArchivedValue::Null(())
        | ArchivedValue::Sequence(_)
        | ArchivedValue::String(_)
        | ArchivedValue::Time(_) => None,
    };
    assert_eq!(offset, Some(-4_i32 * 3_600_i32));
    let back = deserialize(archived);
    assert_eq!(back, parsed.document);
}

#[test]
fn each_value_reads_back_from_rkyv_and_a_date_time_keeps_its_offset() {
    let date = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
    let nanoseconds = NaiveTime::from_hms_nano_opt(9, 0, 0, 123_456_789).unwrap();
    let leap_second = NaiveTime::from_hms_nano_opt(23, 59, 59, 1_500_000_000).unwrap();
    let local = date.and_time(nanoseconds);
    let at = FixedOffset::west_opt(4 * 3600)
        .unwrap()
        .from_local_datetime(&date.and_hms_opt(9, 0, 0).unwrap())
        .single()
        .unwrap();
    let inner: Mapping = IndexMap::from([("a".to_owned(), Value::Integer(1))]);
    let mapping: Mapping = IndexMap::from([
        ("boolean".to_owned(), Value::Boolean(true)),
        ("date".to_owned(), Value::Date(date)),
        ("date-time".to_owned(), Value::DateTime(local)),
        ("float".to_owned(), Value::Float(0.5)),
        ("integer".to_owned(), Value::Integer(-7)),
        ("mapping".to_owned(), Value::Mapping(inner)),
        ("null".to_owned(), Value::Null(())),
        ("offset-date-time".to_owned(), Value::OffsetDateTime(at)),
        (
            "sequence".to_owned(),
            Value::Sequence(vec![Value::String("a".to_owned()), Value::Null(())]),
        ),
        ("string".to_owned(), Value::String("text".to_owned())),
        ("time".to_owned(), Value::Time(leap_second)),
    ]);
    let document = with_frontmatter(mapping);
    let bytes = to_bytes(&document);
    let archived = rkyv::access::<ArchivedRawDocument, Error>(&bytes).unwrap();
    let back = deserialize(archived);
    assert_eq!(back, document);
    // A `DateTime` equals another at the same instant whatever its offset.
    assert!(matches!(
        frontmatter_value(&back, "offset-date-time"),
        Value::OffsetDateTime(read) if read.to_rfc3339() == "2026-10-20T09:00:00-04:00"
    ));
}

#[test]
fn a_tree_at_the_caps_reads_back_from_rkyv() {
    on_two_megabytes(|| {
        let mut source = String::from("---\n");
        for level in 0..VALUE_CAP {
            source.push_str(&"  ".repeat(level));
            source.push_str("a:\n");
        }
        source.push_str(&"  ".repeat(VALUE_CAP));
        source.push_str("b: 1\n---\n");
        source.push_str(&"- ".repeat(BLOCK_CAP - 1));
        source.push_str("a\n");
        let document = valid(&source);
        assert_eq!(
            mapping_depth(document.frontmatter.as_ref().unwrap()),
            VALUE_CAP
        );
        assert_eq!(list_depth(&document.body), BLOCK_CAP - 1);
        reads_back_from_rkyv(RawDocument::Valid(document));
    });
}

#[test]
fn bytes_with_a_byte_changed_in_the_middle_do_not_read() {
    let source = format!("{}\n", "word ".repeat(200));
    let mut bytes = to_bytes(&parse(&source).document);
    let middle = bytes.len() / 2;
    // The middle byte falls in the paragraph's text, and 0xff is never UTF-8.
    bytes[middle] = 0xff;
    let read = rkyv::access::<ArchivedRawDocument, Error>(&bytes);
    assert!(read.is_err());
}
