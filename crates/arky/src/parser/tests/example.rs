//! One document checked node by node against a tree written out by hand,
//! with the byte range of every node and of every frontmatter value its
//! reader reads.

use core::ops::Range;

use chrono::NaiveDate;
use indexmap::IndexMap;

use crate::parser::ast::{self, Continues, FrontmatterFormat, Kind, LinkForm, RawDocument};
use crate::parser::extension::Markdown;
use crate::parser::located::{
    Block, Document, Frontmatter, Heading, Inline, Item, Line, Link, List, Mapping, MarkdownBlock,
    Pair, Paragraph, Scalar, ScalarValue, Section, Sequence, Text, Value,
};
use crate::parser::{ParseOptions, Parsed, parse, parse_located, toml};

const SOURCE: &str = r#"+++
type = "task"
due = 2026-10-10
tags = ["parser", "design"]
priority = 2
weight = 0.5
done = false
+++
# Ship parser

Depends on [[grammar]] and the [spec](spec.md).

- define tree
  - spans
- write rules
"#;

const TOML_PAIRS: &str = r#"type = "task"
due = 2026-10-10
tags = ["parser", "design"]
priority = 2
weight = 0.5
done = false"#;

/// The range starting at byte `start` of [`SOURCE`], checked to hold `covered`.
fn span(start: usize, covered: &str) -> Range<usize> {
    let end = start + covered.len();
    assert_eq!(&SOURCE[start..end], covered);
    start..end
}

fn pair(key: &str, range: Range<usize>, value: Value) -> (String, Pair) {
    let pair = Pair {
        repeats: Vec::new(),
        span: range,
        value,
    };
    (key.to_owned(), pair)
}

fn scalar(range: Range<usize>, value: ScalarValue) -> Value {
    Value::Scalar(Scalar { span: range, value })
}

fn string(range: Range<usize>, value: &str) -> Value {
    scalar(range, ScalarValue::String(value.to_owned()))
}

fn text(start: usize, value: &str) -> Inline {
    Inline::Text(Text {
        kind: Kind(None),
        span: span(start, value),
        value: value.to_owned(),
    })
}

fn paragraph(start: usize, value: &str) -> Block {
    Block::Paragraph(Paragraph {
        kind: Kind(None),
        lines: vec![Line {
            children: vec![text(start, value)],
            kind: Kind(None),
            span: span(start, value),
        }],
        span: span(start, value),
    })
}

/// A list item's content: one section with no heading, holding `blocks`.
fn content(start: usize, covered: &str, blocks: Vec<Block>) -> MarkdownBlock {
    MarkdownBlock {
        kind: Kind(None),
        sections: vec![Section {
            blocks,
            heading: None,
            kind: Kind(None),
            span: span(start, covered),
            subsections: Vec::new(),
        }],
        span: span(start, covered),
    }
}

/// The frontmatter's mapping as its reader reads it.
fn mapping() -> Mapping {
    let due = ScalarValue::Date(NaiveDate::from_ymd_opt(2026, 10, 10).unwrap());
    let tags = Value::Sequence(Sequence {
        items: vec![
            string(span(43, r#""parser""#), "parser"),
            string(span(53, r#""design""#), "design"),
        ],
        span: span(42, r#"["parser", "design"]"#),
    });
    let pairs = IndexMap::from([
        pair(
            "type",
            span(4, r#"type = "task""#),
            string(span(11, r#""task""#), "task"),
        ),
        pair(
            "due",
            span(18, "due = 2026-10-10"),
            scalar(span(24, "2026-10-10"), due),
        ),
        pair("tags", span(35, r#"tags = ["parser", "design"]"#), tags),
        pair(
            "priority",
            span(63, "priority = 2"),
            scalar(span(74, "2"), ScalarValue::Integer(2)),
        ),
        pair(
            "weight",
            span(76, "weight = 0.5"),
            scalar(span(85, "0.5"), ScalarValue::Float(0.5)),
        ),
        pair(
            "done",
            span(89, "done = false"),
            scalar(span(96, "false"), ScalarValue::Boolean(false)),
        ),
    ]);
    Mapping {
        pairs,
        span: span(4, TOML_PAIRS),
    }
}

fn frontmatter() -> Frontmatter {
    Frontmatter {
        dropped: Vec::new(),
        format: FrontmatterFormat::Toml,
        mapping: mapping().into_model(),
        span: span(0, &format!("+++\n{TOML_PAIRS}\n+++")),
    }
}

fn dependencies() -> Block {
    let written = "Depends on [[grammar]] and the [spec](spec.md).";
    Block::Paragraph(Paragraph {
        kind: Kind(None),
        lines: vec![Line {
            children: vec![
                text(121, "Depends on "),
                Inline::Link(Link {
                    children: Vec::new(),
                    continues: Continues::Neither,
                    form: LinkForm::Wiki,
                    kind: Kind(None),
                    span: span(132, "[[grammar]]"),
                    target: "grammar".to_owned(),
                    title: None,
                }),
                text(143, " and the "),
                Inline::Link(Link {
                    children: vec![text(153, "spec")],
                    continues: Continues::Neither,
                    form: LinkForm::Markdown,
                    kind: Kind(None),
                    span: span(152, "[spec](spec.md)"),
                    target: "spec.md".to_owned(),
                    title: None,
                }),
                text(167, "."),
            ],
            kind: Kind(None),
            span: span(121, written),
        }],
        span: span(121, written),
    })
}

fn steps() -> Block {
    Block::List(List {
        items: vec![
            Item {
                body: content(
                    172,
                    "define tree\n  - spans",
                    vec![
                        paragraph(172, "define tree"),
                        Block::List(List {
                            items: vec![Item {
                                body: content(188, "spans", vec![paragraph(188, "spans")]),
                                kind: Kind(None),
                                span: span(186, "- spans"),
                            }],
                            kind: Kind(None),
                            loose: false,
                            span: span(186, "- spans"),
                            start: None,
                        }),
                    ],
                ),
                kind: Kind(None),
                span: span(170, "- define tree\n  - spans"),
            },
            Item {
                body: content(196, "write rules", vec![paragraph(196, "write rules")]),
                kind: Kind(None),
                span: span(194, "- write rules"),
            },
        ],
        kind: Kind(None),
        loose: false,
        span: span(170, "- define tree\n  - spans\n- write rules"),
        start: None,
    })
}

#[test]
fn the_example_parses_into_the_tree_written_by_hand() {
    let section = Section {
        blocks: vec![dependencies(), steps()],
        heading: Some(Heading {
            children: vec![text(108, "Ship parser")],
            kind: Kind(None),
            span: span(106, "# Ship parser"),
        }),
        kind: Kind(None),
        span: span(106, SOURCE[106..].trim_end()),
        subsections: Vec::new(),
    };
    let document = Document {
        body: MarkdownBlock {
            kind: Kind(None),
            sections: vec![section],
            span: span(106, &SOURCE[106..]),
        },
        frontmatter: Some(Ok(frontmatter())),
        kind: Kind(None),
        span: span(0, SOURCE),
    };

    assert_eq!(
        parse_located::<Markdown>(SOURCE, &ParseOptions::default()),
        document
    );
    let model = ast::Document {
        body: document.body.into_model(),
        frontmatter: Some(mapping().into_model()),
        kind: Kind(None),
    };
    assert_eq!(
        parse(SOURCE),
        Parsed {
            document: RawDocument::Valid(model),
            dropped: Vec::new(),
        }
    );
    let content = span(4, &format!("{TOML_PAIRS}\n"));
    assert_eq!(toml::read(SOURCE, content).unwrap().0, mapping());
}
