//! The record: the sample's record, its files, each kind of frontmatter value
//! stored and read back, the fixtures, an invalid document, records that do
//! not read, the depth, the filter, the revision, and the claims and records
//! written before they named a save.

use alloc::collections::BTreeMap;
use std::thread;

use arky_core::parser::ast::{InvalidDocument, Mapping, RawDocument, Value};
use arky_core::parser::{from_json, parse, to_json};
use bson::oid::ObjectId;
use bson::{Bson, DateTime as BsonDateTime, Document, doc};
use chrono::{DateTime, NaiveDate, NaiveTime, Timelike as _, Utc};

use super::{
    Claim, StoredDocument, StoredObject, depth, from_record, frontmatter_is, revision_of, to_record,
};
use crate::StoreError;

const FIXTURES: [&str; 21] = [
    include_str!("../../../../arky-core/src/parser/tests/fixtures/api_reference.md"),
    include_str!("../../../../arky-core/src/parser/tests/fixtures/book_page.md"),
    include_str!("../../../../arky-core/src/parser/tests/fixtures/changelog.md"),
    include_str!("../../../../arky-core/src/parser/tests/fixtures/contact.md"),
    include_str!("../../../../arky-core/src/parser/tests/fixtures/daily_note.md"),
    include_str!("../../../../arky-core/src/parser/tests/fixtures/docusaurus_page.md"),
    include_str!("../../../../arky-core/src/parser/tests/fixtures/essay.md"),
    include_str!("../../../../arky-core/src/parser/tests/fixtures/hugo_post.md"),
    include_str!("../../../../arky-core/src/parser/tests/fixtures/hugo_post_with_table.md"),
    include_str!("../../../../arky-core/src/parser/tests/fixtures/issue_template.md"),
    include_str!("../../../../arky-core/src/parser/tests/fixtures/jekyll_post.md"),
    include_str!("../../../../arky-core/src/parser/tests/fixtures/meeting_notes.md"),
    include_str!("../../../../arky-core/src/parser/tests/fixtures/obsidian_note.md"),
    include_str!("../../../../arky-core/src/parser/tests/fixtures/pandoc_paper.md"),
    include_str!("../../../../arky-core/src/parser/tests/fixtures/readme.md"),
    include_str!("../../../../arky-core/src/parser/tests/fixtures/recipe.md"),
    include_str!("../../../../arky-core/src/parser/tests/fixtures/slash_command.md"),
    include_str!("../../../../arky-core/src/parser/tests/fixtures/task.md"),
    include_str!("../../../../arky-core/src/parser/tests/fixtures/toml_multi_line_string.md"),
    include_str!("../../../../arky-core/src/parser/tests/fixtures/yaml_list_of_mappings.md"),
    include_str!("../../../../arky-core/src/parser/tests/fixtures/zola_page.md"),
];

const ID: &str = "6ad42f681472f05ab2ed9be7";

const SAVE: &str = "6ad42f681472f05ab2ed9be8";

/// A TOML frontmatter holding each kind of value a record stores.
const SAMPLE: &str = r#"+++
title = "Notes"
status = "open"
due = 2026-10-20
priority = 2
ratio = 0.5
draft = false
big = inf
tags = ["parser", "design"]
starts = 07:32:00
offset = 2026-10-20T09:00:00+02:00
local = 2026-10-20T09:00:00

[owner]
name = "Ada"
+++
"#;

fn data(record: &Document) -> &Document {
    record
        .get_document("doc")
        .unwrap()
        .get_document("data")
        .unwrap()
}

fn data_mut(record: &mut Document) -> &mut Document {
    record
        .get_document_mut("doc")
        .unwrap()
        .get_document_mut("data")
        .unwrap()
}

fn date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, 20).unwrap()
}

fn frontmatter(stored: &StoredDocument) -> Option<&Mapping> {
    let RawDocument::Valid(document) = &stored.document else {
        return None;
    };
    document.frontmatter.as_ref()
}

fn instant(text: &str) -> BsonDateTime {
    BsonDateTime::parse_rfc3339_str(text).unwrap()
}

fn invalid(document: &RawDocument) -> Option<&InvalidDocument> {
    let RawDocument::Invalid(invalid) = document else {
        return None;
    };
    Some(invalid)
}

fn mapping(pairs: Vec<(&str, Value)>) -> Mapping {
    pairs
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect()
}

/// A file whose frontmatter holds `count` mappings, each inside the one before.
fn nested_mappings(count: usize) -> StoredDocument {
    let mut value = Value::Null(());
    for _ in 0..count {
        value = Value::Mapping(mapping(vec![("a", value)]));
    }
    with_frontmatter(mapping(vec![("a", value)]))
}

/// Runs `test` on a thread with a 2 MB stack, the stack a test thread gets.
fn on_two_megabytes<F>(test: F)
where
    F: FnOnce() + Send + 'static,
{
    thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(test)
        .unwrap()
        .join()
        .unwrap();
}

/// A paragraph inside `count` list items, each inside the one before.
fn nested_items(count: usize) -> String {
    let mut text = String::new();
    for level in 0..count {
        text.push_str(&"  ".repeat(level));
        text.push_str("- item\n");
    }
    text
}

fn offset(text: &str) -> Value {
    Value::OffsetDateTime(DateTime::parse_from_rfc3339(text).unwrap())
}

/// The tree `document` keeps once stored: each node's `kind` left out, as
/// JSON leaves it out.
fn plain_tree(document: &RawDocument) -> RawDocument {
    from_json(&to_json(document).unwrap()).unwrap()
}

fn stored(path: &str, text: &str) -> StoredDocument {
    StoredDocument {
        document: parse(text).document,
        files: BTreeMap::new(),
        id: ObjectId::parse_str(ID).unwrap(),
        path: path.to_owned(),
        revision: revision_of(text, &BTreeMap::new()).0,
        save: Some(ObjectId::parse_str(SAVE).unwrap()),
    }
}

fn string(text: &str) -> Value {
    Value::String(text.to_owned())
}

/// What MongoDB gives back for `value`: an offset written as UTC, a date-time
/// at midnight UTC as a date, a time to the millisecond.
fn through_mongodb(value: Value) -> Value {
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
                .map(|(key, item)| (key, through_mongodb(item)))
                .collect(),
        ),
        Value::OffsetDateTime(at) => in_utc(at.to_utc()),
        Value::Sequence(items) => Value::Sequence(items.into_iter().map(through_mongodb).collect()),
        Value::Time(time) => {
            let millisecond = time.nanosecond() / 1_000_000 * 1_000_000;
            Value::Time(time.with_nanosecond(millisecond).unwrap())
        }
        Value::Boolean(_)
        | Value::Date(_)
        | Value::Float(_)
        | Value::Integer(_)
        | Value::Null(())
        | Value::String(_)
        | _ => value,
    }
}

/// A file of one heading whose frontmatter is `frontmatter`.
fn with_frontmatter(frontmatter: Mapping) -> StoredDocument {
    let mut written = stored("notes/notes.md", "# Notes\n");
    if let RawDocument::Valid(document) = &mut written.document {
        document.frontmatter = Some(frontmatter);
    }
    written
}

#[test]
fn the_sample_frontmatter_is_stored_as_the_sample_record() {
    let record = to_record(&stored("notes/task.md", SAMPLE)).unwrap();
    let expected = doc! {
        "doc": {
            "t": "valid",
            "bd": { "se": [] },
            "data": {
                "title": "Notes",
                "status": "open",
                "due": instant("2026-10-20T00:00:00Z"),
                "priority": 2_i64,
                "ratio": 0.5_f64,
                "draft": false,
                "big": "inf",
                "tags": ["parser", "design"],
                "starts": { "t": "time", "v": "07:32:00" },
                "offset": instant("2026-10-20T07:00:00Z"),
                "local": instant("2026-10-20T09:00:00Z"),
                "owner": { "name": "Ada" },
            },
        },
        "files": {},
        "_id": ObjectId::parse_str(ID).unwrap(),
        "path": "notes/task.md",
        "revision": "sha256:b62ae835027b98866bb7c61b2ac1675280396d597fa2722d5d3eaeea82073648",
        "save": ObjectId::parse_str(SAVE).unwrap(),
    };
    assert_eq!(record, expected);
}

#[test]
fn the_sample_record_reads_back_as_the_sample_table_says() {
    let back = from_record(to_record(&stored("notes/task.md", SAMPLE)).unwrap()).unwrap();
    let read = mapping(vec![
        ("title", string("Notes")),
        ("status", string("open")),
        ("due", Value::Date(date())),
        ("priority", Value::Integer(2)),
        ("ratio", Value::Float(0.5)),
        ("draft", Value::Boolean(false)),
        ("big", string("inf")),
        (
            "tags",
            Value::Sequence(vec![string("parser"), string("design")]),
        ),
        (
            "starts",
            Value::Time(NaiveTime::from_hms_opt(7, 32, 0).unwrap()),
        ),
        ("offset", offset("2026-10-20T07:00:00Z")),
        ("local", offset("2026-10-20T09:00:00Z")),
        (
            "owner",
            Value::Mapping(mapping(vec![("name", string("Ada"))])),
        ),
    ]);
    let frontmatter = frontmatter(&back).unwrap();
    assert_eq!(frontmatter, &read);
    for key in ["offset", "local"] {
        assert!(
            matches!(&frontmatter[key], Value::OffsetDateTime(at) if at.offset().local_minus_utc() == 0_i32),
            "{key}"
        );
    }
}

#[test]
fn the_files_of_a_record_are_stored_by_their_paths_in_the_bundle() {
    let mut written = stored("notes/report.md", "![Logo](media/logo.png)\n");
    written.files = BTreeMap::from([("media/logo.png".to_owned(), "3f1a".to_owned())]);
    let record = to_record(&written).unwrap();
    assert_eq!(
        record.get_document("files").unwrap(),
        &doc! { "media/logo.png": "3f1a" }
    );
    assert_eq!(from_record(record).unwrap(), written);
}

#[test]
fn a_record_with_no_files_reads_as_a_document_with_no_files() {
    let written = stored("notes/task.md", "# Task\n");
    let mut record = to_record(&written).unwrap();
    record.remove("files");
    let back = from_record(record).unwrap();
    assert_eq!(back.files, BTreeMap::new());
    assert_eq!(back, written);
}

#[test]
fn a_yaml_null_is_stored_as_bson_null_and_reads_back_as_null() {
    let record = to_record(&stored("notes/null.md", "---\nnothing: null\n---\n")).unwrap();
    assert_eq!(data(&record).get("nothing"), Some(&Bson::Null));
    let back = from_record(record).unwrap();
    let read = mapping(vec![("nothing", Value::Null(()))]);
    assert_eq!(frontmatter(&back), Some(&read));
}

#[test]
fn each_kind_of_value_is_stored_as_plain_bson_and_read_back() {
    let seven = NaiveTime::from_hms_opt(7, 32, 0).unwrap();
    let written = with_frontmatter(mapping(vec![
        ("title", string("Notes")),
        ("priority", Value::Integer(2)),
        ("ratio", Value::Float(0.5)),
        ("done", Value::Boolean(false)),
        ("none", Value::Null(())),
        (
            "tags",
            Value::Sequence(vec![string("parser"), string("design")]),
        ),
        (
            "owner",
            Value::Mapping(mapping(vec![("name", string("Ada"))])),
        ),
        ("due", Value::Date(date())),
        ("offset", offset("2026-10-20T09:00:00+02:00")),
        ("midnight", offset("2026-10-20T00:00:00Z")),
        (
            "local",
            Value::DateTime(date().and_hms_opt(9, 0, 0).unwrap()),
        ),
        ("at", Value::Time(seven)),
        (
            "at_ms",
            Value::Time(NaiveTime::from_hms_micro_opt(7, 32, 0, 250_123).unwrap()),
        ),
    ]));
    let record = to_record(&written).unwrap();
    let expected = doc! {
        "title": "Notes",
        "priority": 2_i64,
        "ratio": 0.5_f64,
        "done": false,
        "none": Bson::Null,
        "tags": ["parser", "design"],
        "owner": { "name": "Ada" },
        "due": instant("2026-10-20T00:00:00Z"),
        "offset": instant("2026-10-20T07:00:00Z"),
        "midnight": instant("2026-10-20T00:00:00Z"),
        "local": instant("2026-10-20T09:00:00Z"),
        "at": { "t": "time", "v": "07:32:00" },
        "at_ms": { "t": "time", "v": "07:32:00.250" },
    };
    assert_eq!(data(&record), &expected);

    let back = from_record(record).unwrap();
    let read = mapping(vec![
        ("title", string("Notes")),
        ("priority", Value::Integer(2)),
        ("ratio", Value::Float(0.5)),
        ("done", Value::Boolean(false)),
        ("none", Value::Null(())),
        (
            "tags",
            Value::Sequence(vec![string("parser"), string("design")]),
        ),
        (
            "owner",
            Value::Mapping(mapping(vec![("name", string("Ada"))])),
        ),
        ("due", Value::Date(date())),
        ("offset", offset("2026-10-20T07:00:00Z")),
        ("midnight", Value::Date(date())),
        ("local", offset("2026-10-20T09:00:00Z")),
        ("at", Value::Time(seven)),
        (
            "at_ms",
            Value::Time(NaiveTime::from_hms_milli_opt(7, 32, 0, 250).unwrap()),
        ),
    ]);
    assert_eq!(frontmatter(&back), Some(&read));
}

#[test]
fn a_file_with_a_frontmatter_is_stored_and_read_back() {
    let text = "---\nstatus: open\ndue: 2026-10-20\n---\n# Task\n";
    let written = stored("notes/task.md", text);
    let record = to_record(&written).unwrap();
    assert_eq!(data(&record).get_str("status").unwrap(), "open");
    assert!(matches!(data(&record).get("due"), Some(Bson::DateTime(_))));
    assert_eq!(from_record(record).unwrap(), written);
}

#[test]
fn each_fixture_reads_back_with_the_changes_mongodb_makes() {
    for source in FIXTURES {
        let written = stored("notes/fixture.md", source);
        let back = from_record(to_record(&written).unwrap()).unwrap();
        let mut expected = written.clone();
        expected.document = plain_tree(&written.document);
        if let RawDocument::Valid(document) = &mut expected.document {
            document.frontmatter = document.frontmatter.take().map(|kept| {
                kept.into_iter()
                    .map(|(key, value)| (key, through_mongodb(value)))
                    .collect()
            });
        }
        assert_eq!(back, expected, "{source}");
    }
}

#[test]
fn an_invalid_document_keeps_its_frontmatter_as_written() {
    let written = stored("notes/broken.md", "---\n: : :\n---\ntext\n");
    let kept = invalid(&written.document).unwrap();
    let record = to_record(&written).unwrap();
    let as_serde = bson::serialize_to_document(&kept.frontmatter).unwrap();
    assert_eq!(data(&record), &as_serde);
    assert_eq!(from_record(record).unwrap(), written);
}

#[test]
fn a_document_of_t_and_v_is_a_time_only_when_v_is_a_time() {
    let lookalike = mapping(vec![("t", string("time")), ("v", string("nine"))]);
    let clock = mapping(vec![("t", string("time")), ("v", string("07:32:00"))]);
    let written = with_frontmatter(mapping(vec![
        ("nine", Value::Mapping(lookalike.clone())),
        ("clock", Value::Mapping(clock)),
    ]));
    let mut record = to_record(&written).unwrap();
    data_mut(&mut record).insert("count", Bson::Int32(3));
    let back = from_record(record).unwrap();
    let read = mapping(vec![
        ("nine", Value::Mapping(lookalike)),
        (
            "clock",
            Value::Time(NaiveTime::from_hms_opt(7, 32, 0).unwrap()),
        ),
        ("count", Value::Integer(3)),
    ]);
    assert_eq!(frontmatter(&back), Some(&read));
}

#[test]
fn a_list_like_a_time_or_a_date_reads_back_as_a_list() {
    let pair = Value::Sequence(vec![string("time"), string("07:32:00")]);
    let instant_text = Value::Sequence(vec![string("2026-10-20T00:00:00Z")]);
    let lists = mapping(vec![("pair", pair), ("instant", instant_text)]);
    let record = to_record(&with_frontmatter(lists.clone())).unwrap();
    let expected = doc! {
        "pair": ["time", "07:32:00"],
        "instant": ["2026-10-20T00:00:00Z"],
    };
    assert_eq!(data(&record), &expected);
    let back = from_record(record).unwrap();
    assert_eq!(frontmatter(&back), Some(&lists));
}

#[test]
fn a_record_that_does_not_read_names_its_id_and_serdes_first_error() {
    let written = with_frontmatter(mapping(vec![("status", string("open"))]));
    let mut foreign = to_record(&written).unwrap();
    data_mut(&mut foreign).insert("x", f64::NAN);
    let no_row = "the record ObjectId(\"6ad42f681472f05ab2ed9be7\") does not read: BSON error. Kind: A deserialization-related error occurred. Message: data did not match any variant of untagged enum Value.";
    assert_eq!(
        from_record(foreign),
        Err(StoreError::Backend(no_row.to_owned()))
    );

    let mut broken = to_record(&written).unwrap();
    broken.remove("path");
    let missing = "the record ObjectId(\"6ad42f681472f05ab2ed9be7\") does not read: BSON error. Kind: A deserialization-related error occurred. Message: missing field `path`.";
    assert_eq!(
        from_record(broken),
        Err(StoreError::Backend(missing.to_owned()))
    );
}

#[test]
fn a_record_holding_a_bson_value_arky_does_not_write_does_not_read() {
    let refused = "the record ObjectId(\"6ad42f681472f05ab2ed9be7\") does not read: BSON error. Kind: A deserialization-related error occurred. Message: data did not match any variant of untagged enum Value.";
    let written = with_frontmatter(mapping(vec![("status", string("open"))]));
    for foreign in [
        Bson::ObjectId(ObjectId::parse_str(ID).unwrap()),
        Bson::Timestamp(bson::Timestamp {
            time: 1,
            increment: 2,
        }),
        Bson::MinKey,
        Bson::Symbol("open".to_owned()),
    ] {
        let mut record = to_record(&written).unwrap();
        data_mut(&mut record).insert("x", foreign.clone());
        assert_eq!(
            from_record(record),
            Err(StoreError::Backend(refused.to_owned())),
            "{foreign:?}"
        );
    }
}

#[test]
fn a_mapping_with_a_key_that_does_not_start_with_a_dollar_reads_back() {
    let mixed = mapping(vec![("$oid", string("a")), ("name", string("Ada"))]);
    let lists = mapping(vec![
        ("x", Value::Mapping(mixed)),
        ("empty", Value::Mapping(Mapping::new())),
    ]);
    let back = from_record(to_record(&with_frontmatter(lists.clone())).unwrap()).unwrap();
    assert_eq!(frontmatter(&back), Some(&lists));
}

#[test]
fn a_hand_built_non_finite_float_is_stored_as_text_and_reads_back_as_text() {
    for (float, written) in [
        (f64::NAN, "nan"),
        (f64::INFINITY, "inf"),
        (f64::NEG_INFINITY, "-inf"),
    ] {
        let record =
            to_record(&with_frontmatter(mapping(vec![("x", Value::Float(float))]))).unwrap();
        assert_eq!(
            data(&record).get("x"),
            Some(&Bson::String(written.to_owned()))
        );
        let back = from_record(record).unwrap();
        assert_eq!(
            frontmatter(&back),
            Some(&mapping(vec![("x", string(written))]))
        );
    }
}

#[test]
fn a_record_deeper_than_mongodb_holds_is_not_stored() {
    assert_eq!(
        depth(&to_record(&stored("notes/one.md", "text\n")).unwrap()),
        11
    );
    let twelve = to_record(&stored("notes/deep.md", &nested_items(12))).unwrap();
    assert_eq!(depth(&twelve), 95);
    let refused = "notes/deep.md nests 102 levels, and MongoDB holds 100";
    assert_eq!(
        to_record(&stored("notes/deep.md", &nested_items(13))),
        Err(StoreError::Backend(refused.to_owned()))
    );
}

#[test]
fn a_frontmatter_value_98_mappings_deep_is_not_stored() {
    on_two_megabytes(|| {
        let deepest = to_record(&nested_mappings(97)).unwrap();
        assert_eq!(depth(&deepest), 100);
        assert_eq!(from_record(deepest).unwrap(), nested_mappings(97));
        let refused = "notes/notes.md nests 101 levels, and MongoDB holds 100";
        assert_eq!(
            to_record(&nested_mappings(98)),
            Err(StoreError::Backend(refused.to_owned()))
        );
    });
}

#[test]
fn a_frontmatter_value_400_mappings_deep_is_refused_before_it_is_written() {
    on_two_megabytes(|| {
        let refused = "notes/notes.md nests 403 levels, and MongoDB holds 100";
        assert_eq!(
            to_record(&nested_mappings(400)),
            Err(StoreError::Backend(refused.to_owned()))
        );
    });
}

#[test]
fn the_filter_on_a_frontmatter_key_names_doc_data() {
    let status = frontmatter_is("status", &string("open")).unwrap();
    assert_eq!(status.to_string(), r#"{ "doc.data.status": "open" }"#);
    let due = frontmatter_is("due", &Value::Date(date())).unwrap();
    assert_eq!(
        due,
        doc! { "doc.data.due": instant("2026-10-20T00:00:00Z") }
    );
}

#[test]
fn the_filter_on_a_frontmatter_key_a_dotted_path_cannot_name_reads_it_by_name() {
    let dotted = frontmatter_is("a.b", &string("x")).unwrap();
    assert_eq!(
        dotted,
        doc! { "$expr": { "$eq": [
            { "$getField": { "field": { "$literal": "a.b" }, "input": "$doc.data" } },
            "x",
        ] } }
    );
    let dollar = frontmatter_is("$price", &Value::Integer(3)).unwrap();
    assert_eq!(
        dollar,
        doc! { "$expr": { "$eq": [
            { "$getField": { "field": { "$literal": "$price" }, "input": "$doc.data" } },
            3_i64,
        ] } }
    );
}

#[test]
fn the_revision_is_the_sha_256_of_the_text_and_of_each_file_each_part_preceded_by_its_length() {
    assert_eq!(
        revision_of("", &BTreeMap::new()).0,
        "sha256:ba768b331fd86cec803be04e56ab2b3d4c0e98ef4ee4fcd4e72ad7cce61a1d1f"
    );
    let logo = BTreeMap::from([("media/logo.png".to_owned(), "abc".to_owned())]);
    assert_eq!(
        revision_of("# Report\n", &logo).0,
        "sha256:b3dba8daff13fe05db0539675fb03aa88556e0d780a5c274135db17d8699c755"
    );
    let other = BTreeMap::from([("media/logo.png".to_owned(), "abd".to_owned())]);
    assert_ne!(
        revision_of("# Report\n", &other),
        revision_of("# Report\n", &logo)
    );
    let file = BTreeMap::from([("b".to_owned(), "c".to_owned())]);
    assert_ne!(
        revision_of("a", &file),
        revision_of("a1:b1:c", &BTreeMap::new())
    );
}

#[test]
fn a_claim_written_before_claims_named_a_save_reads_with_none() {
    let record = doc! {
        "_id": "0123",
        "object": "files/0123",
        "content_type": "image/png",
        "size": 27_i64,
        "description": "Logo",
        "stored": true,
        "embedded_by": [{ "document": "notes/report.md", "path": "notes/media/logo.png" }],
    };
    let object: StoredObject = bson::deserialize_from_document(record).unwrap();
    let claim = Claim {
        document: "notes/report.md".to_owned(),
        path: "notes/media/logo.png".to_owned(),
        save: None,
    };
    assert_eq!(object.embedded_by, [claim]);
}

#[test]
fn a_record_written_before_records_named_a_save_reads_with_none() {
    let written = stored("notes/task.md", "# Task\n");
    let mut record = to_record(&written).unwrap();
    record.remove("save");
    let back = from_record(record).unwrap();
    assert_eq!(back.save, None);
    assert_eq!(back.revision, written.revision);
}
