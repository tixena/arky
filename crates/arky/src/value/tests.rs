//! A serde type written as a `Value` and read back, and a frontmatter read
//! as a serde type.

use core::ops::Range;

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use tixschema::model_schema;

use self::note_schema::DecodeWith as _;
use super::{from_value, to_value};
use crate::parser::ast::{Mapping, RawDocument, Value};
use crate::parser::parse;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "body")]
enum Adjacent {
    Count(u32),
    Pair(String, bool),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
enum Either {
    Number(i64),
    Words(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum External {
    Moved { from: String, to: String },
    Named(String),
    Plain,
    Point(i32, i32),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
enum Internal {
    Book { isbn: String },
    Web { url: String },
}

/// A note's frontmatter, read with the recovering decode.
#[model_schema(enable = ["decode_with"])]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub created: NaiveDate,
    pub due: DateTime<FixedOffset>,
    pub starts: NaiveTime,
    pub tags: Vec<String>,
    pub title: String,
    pub views: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Published {
    on: NaiveDate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Shapes {
    adjacent: Adjacent,
    at: DateTime<Utc>,
    counts: IndexMap<u32, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    due_at: Option<DateTime<FixedOffset>>,
    either: Vec<Either>,
    external: Vec<External>,
    internal: Internal,
    local: NaiveDateTime,
    missing: Option<String>,
    #[serde(flatten)]
    published: Published,
    ratio: f64,
    reply: Result<u32, String>,
    span: Range<usize>,
    time: NaiveTime,
    tuple: (String, i8),
    unit: Result<(), String>,
    views: u64,
}

/// The frontmatter of `source`, which reads.
fn frontmatter(source: &str) -> Option<Value> {
    match parse(source).document {
        RawDocument::Valid(document) => document.frontmatter.map(Value::Mapping),
        RawDocument::Invalid(_) => None,
    }
}

fn note() -> Note {
    Note {
        created: NaiveDate::from_ymd_opt(2026, 10, 17).unwrap(),
        due: "2026-10-20T09:00:00+02:00".parse().unwrap(),
        starts: NaiveTime::from_hms_opt(7, 32, 0).unwrap(),
        tags: vec!["parser".to_owned()],
        title: "Notes".to_owned(),
        views: 7,
    }
}

fn shapes() -> Shapes {
    Shapes {
        adjacent: Adjacent::Pair("x".to_owned(), true),
        at: DateTime::from_timestamp(1_792_231_200, 0).unwrap(),
        counts: IndexMap::from([(7, "seven".to_owned()), (11, "eleven".to_owned())]),
        due_at: Some("2026-10-20T09:00:00+02:00".parse().unwrap()),
        either: vec![Either::Number(3), Either::Words("three".to_owned())],
        external: vec![
            External::Moved {
                from: "a".to_owned(),
                to: "b".to_owned(),
            },
            External::Named("n".to_owned()),
            External::Plain,
            External::Point(1, -2),
        ],
        internal: Internal::Web {
            url: "https://example.com/a".to_owned(),
        },
        local: NaiveDate::from_ymd_opt(1979, 5, 27)
            .unwrap()
            .and_hms_milli_opt(7, 32, 0, 250)
            .unwrap(),
        missing: None,
        published: Published {
            on: NaiveDate::from_ymd_opt(2026, 1, 2).unwrap(),
        },
        ratio: 0.5,
        reply: Err("refused".to_owned()),
        span: 4..9,
        time: NaiveTime::from_hms_opt(9, 30, 0).unwrap(),
        tuple: ("left".to_owned(), -3),
        unit: Ok(()),
        views: 7,
    }
}

#[test]
fn every_shape_serde_writes_reads_back_the_same() {
    let written = shapes();
    let back: Shapes = from_value(&to_value(&written).unwrap()).unwrap();
    assert_eq!(back, written);
}

#[test]
fn a_type_is_the_plain_value_serde_writes() {
    let expected = Value::Mapping(Mapping::from([
        ("created".to_owned(), Value::String("2026-10-17".to_owned())),
        (
            "due".to_owned(),
            Value::String("2026-10-20T09:00:00+02:00".to_owned()),
        ),
        ("starts".to_owned(), Value::String("07:32:00".to_owned())),
        (
            "tags".to_owned(),
            Value::Sequence(vec![Value::String("parser".to_owned())]),
        ),
        ("title".to_owned(), Value::String("Notes".to_owned())),
        ("views".to_owned(), Value::Integer(7)),
    ]));
    assert_eq!(to_value(&note()).unwrap(), expected);
}

#[test]
fn a_frontmatter_written_by_hand_reads_as_a_type() {
    let source = "---\ncreated: 2026-10-17\ndue: 2026-10-20T09:00:00+02:00\nstarts: 07:32:00\n\
                  tags: [parser]\ntitle: Notes\nviews: 7\n---\n";
    let read: Note = from_value(&frontmatter(source).unwrap()).unwrap();
    assert_eq!(read, note());
}

#[test]
fn a_frontmatter_is_read_by_the_recovering_decode_through_its_json_value() {
    let source = "---\ncreated: 2026-10-17\ndue: 2026-10-20T09:00:00+02:00\nstarts: 07:32:00\n\
                  tags: [parser]\ntitle: Notes\nviews: many\n---\n";
    let held = frontmatter(source).unwrap();
    let mut listed = Vec::new();
    let read = Note::from_value_with((&held).into(), |value, issues| {
        listed.extend_from_slice(issues);
        value["views"] = serde_json::Value::from(7_u64);
        note_schema::Verdict::Fixed
    });
    assert_eq!(read.unwrap(), note());
    let views = note_schema::Path(vec![note_schema::Segment::Key("views".to_owned())]);
    assert!(
        matches!(
            listed.as_slice(),
            [note_schema::Issue::Invalid { path, expected: note_schema::Expected::U64, found, reason: _ }]
                if *path == views && *found == "many"
        ),
        "{listed:?}"
    );
}

#[test]
fn a_date_a_time_and_a_date_time_are_the_text_chrono_reads() {
    let held = Value::Sequence(vec![
        Value::Date(NaiveDate::from_ymd_opt(2026, 10, 17).unwrap()),
        Value::Time(NaiveTime::from_hms_milli_opt(7, 32, 0, 250).unwrap()),
        Value::OffsetDateTime("2026-10-20T09:00:00+02:00".parse().unwrap()),
        Value::DateTime(
            NaiveDate::from_ymd_opt(1979, 5, 27)
                .unwrap()
                .and_hms_opt(7, 32, 0)
                .unwrap(),
        ),
    ]);
    assert_eq!(
        serde_json::Value::from(&held),
        serde_json::json!([
            "2026-10-17",
            "07:32:00.250",
            "2026-10-20T09:00:00+02:00",
            "1979-05-27T07:32:00"
        ])
    );
}

#[test]
fn an_integer_outside_64_bits_is_written_as_its_digits() {
    assert_eq!(
        to_value(&u64::MAX).unwrap(),
        Value::String("18446744073709551615".to_owned())
    );
    assert_eq!(
        to_value(&i128::MIN).unwrap(),
        Value::String("-170141183460469231731687303715884105728".to_owned())
    );
}

#[test]
fn a_float_that_is_not_finite_is_written_as_a_float_and_read_as_null() {
    assert_eq!(
        to_value(&f64::NEG_INFINITY).unwrap(),
        Value::Float(f64::NEG_INFINITY)
    );
    assert_eq!(
        serde_json::Value::from(&Value::Float(f64::NAN)),
        serde_json::Value::Null
    );
}

#[test]
fn a_map_key_is_its_text_and_reads_back_as_its_type() {
    let counts = IndexMap::from([(true, 1_u8), (false, 0_u8)]);
    let written = to_value(&counts).unwrap();
    assert_eq!(
        written,
        Value::Mapping(Mapping::from([
            ("true".to_owned(), Value::Integer(1)),
            ("false".to_owned(), Value::Integer(0)),
        ]))
    );
    assert_eq!(from_value::<IndexMap<bool, u8>>(&written).unwrap(), counts);
}

#[test]
fn a_map_key_that_is_a_list_is_refused() {
    let keyed = IndexMap::from([(vec![1_u8], "one")]);
    assert_eq!(
        to_value(&keyed).unwrap_err().to_string(),
        "a map key is text, a number or a boolean"
    );
}
