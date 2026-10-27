//! The tree written to JSON and read back, and the TypeScript its types
//! publish. The JSON holds the markdown's own tree: no node writes its
//! `kind`, and `rebuild_with` builds it again.

use chrono::{DateTime, NaiveDate, NaiveTime};

use super::blocks::{nested_grid_tables, nested_markdown_blocks};
use super::extension::Comments;
use super::{on_two_megabytes, valid_body};
use crate::parser::ast::{
    Args, Block, BlockQuote, Code, CodeBlock, Document, Emphasis, Heading, Html, Image, Inline,
    InvalidDocument, InvalidFrontmatter, Item, Kind, Line, LineBreak, Link, LinkEmphasis,
    LinkInline, LinkStrong, List, Mapping, MarkdownBlock, Paragraph, RawDocument, Section, Strong,
    Table, TableCell, TableRow, Text, ThematicBreak, Value,
};
use crate::parser::error::RebuildError;
use crate::parser::extension::{Markdown, Plain, With};
use crate::parser::inline::INLINE_CAP;
use crate::parser::lexer::BLOCK_CAP;
use crate::parser::{ParseOptions, from_json, parse, parse_with, rebuild_with, to_json};
use crate::writer::{WriteOptions, write};

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

/// Parses `source`, writes the tree to JSON and reads it back: the same
/// tree, every `kind` empty.
pub(super) fn reads_back_from_json(source: &str) {
    let mut parsed = parse(source).document;
    let back: RawDocument = from_json(&to_json(&parsed).unwrap()).unwrap();
    unclaim(&mut parsed);
    assert_eq!(back, parsed);
}

/// Parses `source` with `Comments`, writes the tree to JSON, reads it back
/// and builds its extension nodes again: the tree that `source` written in
/// arky's form, its frontmatter in YAML, parses into.
pub(super) fn rebuilds_from_json(source: &str) {
    let options = ParseOptions::default();
    let parsed = parse_with::<Comments>(source, &options).unwrap().document;
    let back: RawDocument<With<Comments>> = from_json(&to_json(&parsed).unwrap()).unwrap();
    let text = write(&parsed, &WriteOptions::default()).unwrap();
    let written = parse_with::<Comments>(&text, &options).unwrap().document;
    assert_eq!(rebuild_with::<Comments>(&back, &options).unwrap(), written);
}

#[test]
fn the_sample_document_reads_back_from_json() {
    let document = parse("---\ntitle: Notes\n---\n# A\ntext\n").document;
    let json = to_json(&document).unwrap();
    assert_eq!(
        json,
        concat!(
            r#"{"t":"valid","bd":{"se":[{"bl":[{"t":"paragraph","li":[{"ch":[{"t":"text","#,
            r#""v":"text"}]}]}],"hd":{"ch":[{"t":"text","v":"A"}]},"ss":[]}]},"#,
            r#""data":{"title":"Notes"}}"#,
        )
    );
    let back: RawDocument = from_json(&json).unwrap();
    assert_eq!(back, document);
}

#[test]
fn the_json_names_each_field_short_and_each_tag_in_full() {
    let document = parse("---\nstatus: open\n---\n# Task\n\ntext\n").document;
    let json = to_json(&document).unwrap();
    assert_eq!(
        json,
        concat!(
            r#"{"t":"valid","bd":{"se":[{"bl":[{"t":"paragraph","li":[{"ch":[{"t":"text","#,
            r#""v":"text"}]}]}],"hd":{"ch":[{"t":"text","v":"Task"}]},"ss":[]}]},"#,
            r#""data":{"status":"open"}}"#,
        )
    );
    let back: RawDocument = from_json(&json).unwrap();
    assert_eq!(back, document);
}

#[test]
fn the_entries_a_frontmatter_drops_are_listed_beside_the_document_and_not_written_to_json() {
    let source = "---\ntitle: Notes\ntags: [a, b\ndate: 2026-10-17\n---\n# Body\n";
    let parsed = parse(source);
    let spans: Vec<&str> = parsed
        .dropped
        .iter()
        .map(|drop| &source[drop.span.clone()])
        .collect();
    assert_eq!(spans, ["tags: [a, b"]);
    let json = to_json(&parsed.document).unwrap();
    assert!(
        !json.contains("tags") && !json.contains("message"),
        "{json}"
    );
    let back: RawDocument = from_json(&json).unwrap();
    assert_eq!(back, parsed.document);
    let definition = <Document>::ts_definition();
    assert!(!definition.contains("dropped"), "{definition}");
}

#[test]
fn an_invalid_document_is_written_to_json_and_read_back() {
    let source = "---\n{a: 1, b: [}\n---\n# A\n";
    let document = parse(source).document;
    let json = to_json(&document).unwrap();
    assert!(
        json.starts_with(r#"{"t":"invalid","bd":"#)
            && json.contains(concat!(
                r#""data":{"er":"expected `,` or `]` in the list","#,
                r#""fo":"yaml","tx":"{a: 1, b: [}"}"#,
            )),
        "{json}"
    );
    let back: RawDocument = from_json(&json).unwrap();
    assert_eq!(back, document);
    reads_back_from_json(source);
    rebuilds_from_json(source);
    let definition = <RawDocument>::ts_definition();
    assert!(
        definition.contains("t: \"invalid\";\n} & InvalidDocument")
            && definition.contains("t: \"valid\";\n} & Document<E>;"),
        "{definition}"
    );
    let invalid = InvalidDocument::ts_definition();
    let frontmatter = InvalidFrontmatter::ts_definition();
    assert!(
        invalid.contains("bd: MarkdownBlock<Markdown>;")
            && invalid.contains("data: InvalidFrontmatter;"),
        "{invalid}"
    );
    assert!(
        frontmatter.contains("er: string;")
            && frontmatter.contains("fo: FrontmatterFormat;")
            && frontmatter.contains("tx: string;"),
        "{frontmatter}"
    );
}

#[test]
fn a_tree_at_the_caps_reads_back_from_json() {
    on_two_megabytes(|| {
        let stars = "*".repeat(2 * INLINE_CAP);
        for source in [
            format!("{stars}a{stars}\n"),
            format!("{}a\n", "- ".repeat(BLOCK_CAP)),
            nested_grid_tables(BLOCK_CAP),
        ] {
            reads_back_from_json(&source);
        }
        let options = ParseOptions::default();
        let parsed = parse_with::<Plain>(&nested_markdown_blocks(BLOCK_CAP), &options)
            .unwrap()
            .document;
        let back: RawDocument<With<Plain>> = from_json(&to_json(&parsed).unwrap()).unwrap();
        assert_eq!(rebuild_with::<Plain>(&back, &options).unwrap(), parsed);
    });
}

#[test]
fn json_that_is_not_a_document_does_not_read() {
    for text in ["{}", r#"{"t":"valid"}"#] {
        let read: Result<RawDocument, _> = from_json(text);
        assert!(read.is_err(), "{text}");
    }
    let read: Result<RawDocument, _> = from_json(r#"{"t":"valid"}"#);
    let error = read.unwrap_err().to_string();
    assert!(error.starts_with("missing field `bd`"), "{error}");
}

#[test]
fn a_link_whose_text_holds_a_link_does_not_read_from_json() {
    let json = concat!(
        r#"{"t":"valid","bd":{"se":[{"bl":[{"t":"paragraph","li":[{"ch":[{"t":"link","#,
        r#""ch":[{"t":"link","ch":[{"t":"text","v":"x"}],"co":"neither","#,
        r#""fr":"markdown","tg":"https://a.example"}],"co":"neither","fr":"markdown","#,
        r#""tg":"https://b.example"}]}]}],"ss":[]}]}}"#,
    );
    let error = from_json::<Markdown>(json).unwrap_err().to_string();
    assert!(error.contains("unknown variant `link`"), "{error}");
}

#[test]
fn a_link_holding_an_emphasis_and_a_strong_writes_the_json_it_wrote_before() {
    assert_eq!(
        to_json(&parse("[*a* and **b**](u)\n").document).unwrap(),
        concat!(
            r#"{"t":"valid","bd":{"se":[{"bl":[{"t":"paragraph","li":[{"ch":[{"t":"link","#,
            r#""ch":[{"t":"emphasis","ch":[{"t":"text","v":"a"}],"co":"neither"},"#,
            r#"{"t":"text","v":" and "},{"t":"strong","ch":[{"t":"text","v":"b"}],"#,
            r#""co":"neither"}],"co":"neither","fr":"markdown","tg":"u"}]}]}],"ss":[]}]}}"#,
        )
    );
}

#[test]
fn no_node_is_written_with_a_span() {
    let json = to_json(&parse("  text\n").document).unwrap();
    assert!(json.contains(r#"{"t":"text","v":"text"}"#), "{json}");
    assert!(!json.contains("span"), "{json}");
}

#[test]
fn the_claim_pass_builds_a_claimed_block_again_from_json() {
    let source = "```markdown\n# Inner\n```\n";
    let options = ParseOptions::default();
    let parsed = parse_with::<Plain>(source, &options).unwrap().document;
    let back: RawDocument<With<Plain>> = from_json(&to_json(&parsed).unwrap()).unwrap();
    let claimed = |document: &RawDocument<With<Plain>>| {
        matches!(
            &valid_body(document).unwrap().sections[0].blocks[0],
            Block::CodeBlock(block) if block.kind.0.is_some()
        )
    };
    assert!(claimed(&parsed));
    assert!(!claimed(&back));
    assert_eq!(rebuild_with::<Plain>(&back, &options).unwrap(), parsed);
}

#[test]
fn a_toml_frontmatter_holding_a_null_is_rebuilt_in_yaml() {
    let options = ParseOptions::default();
    let source = "+++\ntitle = \"Notes\"\n+++\n```markdown\n# A\n```\n";
    let mut parsed = parse_with::<Plain>(source, &options).unwrap().document;
    if let RawDocument::Valid(valid) = &mut parsed
        && let Some(mapping) = &mut valid.frontmatter
    {
        mapping["title"] = Value::Null(());
    }
    let back: RawDocument<With<Plain>> = from_json(&to_json(&parsed).unwrap()).unwrap();
    let rebuilt = rebuild_with::<Plain>(&back, &options).unwrap();
    let yaml = write(&parsed, &WriteOptions::default()).unwrap();
    assert!(yaml.starts_with("---\ntitle: null\n---\n"), "{yaml}");
    let written = parse_with::<Plain>(&yaml, &options).unwrap().document;
    assert_eq!(rebuilt, written);
}

#[test]
fn a_null_in_the_toml_args_of_a_code_block_fails_the_rebuild_naming_the_key() {
    let options = ParseOptions::default();
    let mut parsed = parse_with::<Plain>("```sh a = 1, b = 2\n```\n", &options)
        .unwrap()
        .document;
    if let RawDocument::Valid(valid) = &mut parsed
        && let Block::CodeBlock(code) = &mut valid.body.sections[0].blocks[0]
        && let Args::Toml(mapping) = &mut code.args
    {
        mapping["a"] = Value::Null(());
    }
    let back: RawDocument<With<Plain>> = from_json(&to_json(&parsed).unwrap()).unwrap();
    let rebuilt = rebuild_with::<Plain>(&back, &options);
    assert!(
        matches!(&rebuilt, Err(RebuildError::Write(error)) if error.key == "a"),
        "{rebuilt:?}"
    );
}

#[test]
fn the_sample_frontmatter_is_written_to_json_as_plain_values_and_read_back() {
    let document = parse(SAMPLE).document;
    let json = to_json(&document).unwrap();
    assert_eq!(
        json,
        concat!(
            r#"{"t":"valid","bd":{"se":[]},"data":{"title":"Notes","status":"open","#,
            r#""due":{"$date":{"$numberLong":"1792454400000"}},"priority":2,"ratio":0.5,"#,
            r#""draft":false,"big":"inf","tags":["parser","design"],"#,
            r#""starts":{"t":"time","v":"07:32:00"},"#,
            r#""offset":{"$date":{"$numberLong":"1792479600000"}},"#,
            r#""local":{"$date":{"$numberLong":"1792486800000"}},"owner":{"name":"Ada"}}}"#,
        )
    );
    let back: RawDocument = from_json(&json).unwrap();
    let in_utc = |text: &str| Value::OffsetDateTime(DateTime::parse_from_rfc3339(text).unwrap());
    let read = mapping(vec![
        ("title", text("Notes")),
        ("status", text("open")),
        (
            "due",
            Value::Date(NaiveDate::from_ymd_opt(2026, 10, 20).unwrap()),
        ),
        ("priority", Value::Integer(2)),
        ("ratio", Value::Float(0.5)),
        ("draft", Value::Boolean(false)),
        ("big", text("inf")),
        (
            "tags",
            Value::Sequence(vec![text("parser"), text("design")]),
        ),
        (
            "starts",
            Value::Time(NaiveTime::from_hms_opt(7, 32, 0).unwrap()),
        ),
        ("offset", in_utc("2026-10-20T07:00:00Z")),
        ("local", in_utc("2026-10-20T09:00:00Z")),
        (
            "owner",
            Value::Mapping(mapping(vec![("name", text("Ada"))])),
        ),
    ]);
    let frontmatter = frontmatter(&back);
    assert_eq!(frontmatter, &read);
    for key in ["offset", "local"] {
        assert!(
            matches!(&frontmatter[key], Value::OffsetDateTime(at) if at.offset().local_minus_utc() == 0_i32),
            "{key}"
        );
    }
}

#[test]
fn a_null_is_written_to_json_as_null_and_read_back() {
    let document = parse("---\nnothing: null\n---\n").document;
    let json = to_json(&document).unwrap();
    assert!(json.ends_with(r#""data":{"nothing":null}}"#), "{json}");
    let back: RawDocument = from_json(&json).unwrap();
    assert_eq!(scalar(&back, "nothing"), Value::Null(()));
}

#[test]
fn a_non_finite_literal_is_written_to_json_as_its_text_and_read_back() {
    for (source, literal) in [
        ("---\nx: .nan\n---\n", ".nan"),
        ("---\nx: .inf\n---\n", ".inf"),
        ("---\nx: -.inf\n---\n", "-.inf"),
        ("+++\nx = nan\n+++\n", "nan"),
        ("+++\nx = -inf\n+++\n", "-inf"),
    ] {
        let document = parse(source).document;
        let json = to_json(&document).unwrap();
        assert!(json.contains(&format!(r#""x":"{literal}""#)), "{json}");
        let back: RawDocument = from_json(&json).unwrap();
        assert_eq!(back, document);
        assert_eq!(scalar(&back, "x"), text(literal));
    }
}

#[test]
fn a_hand_built_non_finite_float_is_written_to_json_as_text_and_reads_back_as_text() {
    for (float, written) in [
        (f64::NAN, "nan"),
        (f64::INFINITY, "inf"),
        (f64::NEG_INFINITY, "-inf"),
    ] {
        let mut document = parse("---\nx: 1\n---\n").document;
        if let RawDocument::Valid(valid) = &mut document {
            valid.frontmatter = Some(mapping(vec![("x", Value::Float(float))]));
        }
        let json = to_json(&document).unwrap();
        assert!(
            json.ends_with(&format!(r#""data":{{"x":"{written}"}}}}"#)),
            "{json}"
        );
        let back: RawDocument = from_json(&json).unwrap();
        assert_eq!(scalar(&back, "x"), text(written));
    }
}

#[test]
fn a_lookalike_of_a_time_or_a_date_reads_back_from_json_as_itself() {
    let seven = Value::Time(NaiveTime::from_hms_opt(7, 32, 0).unwrap());
    let nine = mapping(vec![("t", text("time")), ("v", text("nine"))]);
    let extra = mapping(vec![
        ("t", text("time")),
        ("v", text("07:32:00")),
        ("w", text("x")),
    ]);
    let pair = Value::Sequence(vec![text("time"), text("07:32:00")]);
    let instant = Value::Sequence(vec![text("2026-10-20T00:00:00Z")]);
    let mut document = parse("---\nx: 1\n---\n").document;
    if let RawDocument::Valid(valid) = &mut document {
        valid.frontmatter = Some(mapping(vec![
            ("clock", seven),
            ("nine", Value::Mapping(nine)),
            ("extra", Value::Mapping(extra)),
            ("pair", pair),
            ("instant", instant),
        ]));
    }
    let back: RawDocument = from_json(&to_json(&document).unwrap()).unwrap();
    assert_eq!(back, document);
}

#[test]
fn a_document_of_dollar_keys_alone_does_not_read_from_json() {
    let json = r#"{"t":"valid","bd":{"se":[]},"data":{"x":{"$oid":"6ad42f681472f05ab2ed9be7"}}}"#;
    let read: Result<RawDocument, _> = from_json(json);
    read.unwrap_err();
}

#[test]
fn the_typescript_of_a_value_is_an_untagged_union_with_no_non_finite_float() {
    let value = Value::ts_definition();
    assert!(
        value.contains("export type Value = boolean | bigint | number | string | "),
        "{value}"
    );
    assert!(!value.contains("NonFinite"), "{value}");
}

#[test]
fn the_typescript_of_a_document_names_its_nodes() {
    let raw = <RawDocument>::ts_definition();
    let document = <Document>::ts_definition();
    let block = <MarkdownBlock>::ts_definition();
    assert!(raw.contains("t: \"valid\";\n} & Document<E>;"), "{raw}");
    assert!(document.contains("bd: MarkdownBlock<E>;"), "{document}");
    assert!(
        document.contains("data: Mapping | undefined;"),
        "{document}"
    );
    assert!(block.contains("se: Array<Section<E>>;"), "{block}");
}

#[test]
fn the_zod_schema_of_a_document_names_its_fields_short_and_its_tags_in_full() {
    let raw = <RawDocument>::zod_schema();
    let document = <Document>::zod_schema();
    let value = Value::zod_schema();
    assert!(
        raw.contains("t: z.literal(\"invalid\"),") && raw.contains("t: z.literal(\"valid\"),"),
        "{raw}"
    );
    assert!(
        document.contains("get bd() {") && document.contains("get data() {"),
        "{document}"
    );
    assert!(
        value.contains("z.union([z.boolean(), z.codec(z.union([z.bigint(), z.number().int()]), "),
        "{value}"
    );
}

#[test]
fn no_node_writes_a_span_or_its_kind() {
    let definitions = [
        <RawDocument>::ts_definition(),
        InvalidDocument::ts_definition(),
        InvalidFrontmatter::ts_definition(),
        <Document>::ts_definition(),
        <MarkdownBlock>::ts_definition(),
        <BlockQuote>::ts_definition(),
        <Html>::ts_definition(),
        <ThematicBreak>::ts_definition(),
        <Section>::ts_definition(),
        <Heading>::ts_definition(),
        <Paragraph>::ts_definition(),
        <Line>::ts_definition(),
        <List>::ts_definition(),
        <Item>::ts_definition(),
        <CodeBlock>::ts_definition(),
        <Table>::ts_definition(),
        <TableRow>::ts_definition(),
        <TableCell>::ts_definition(),
        <Text>::ts_definition(),
        <Emphasis>::ts_definition(),
        <Strong>::ts_definition(),
        <LineBreak>::ts_definition(),
        <Link>::ts_definition(),
        <LinkEmphasis>::ts_definition(),
        <LinkStrong>::ts_definition(),
        <Image>::ts_definition(),
        <Code>::ts_definition(),
        Value::ts_definition(),
    ];
    for definition in definitions {
        assert!(
            !definition.contains("span:") && !definition.contains("content:"),
            "{definition}"
        );
        assert!(!definition.contains("kind"), "{definition}");
    }
}

/// The frontmatter of `document`, a valid document with one.
fn frontmatter(document: &RawDocument) -> &Mapping {
    let mapping = match document {
        RawDocument::Invalid(_) => None,
        RawDocument::Valid(valid) => valid.frontmatter.as_ref(),
    };
    mapping.unwrap()
}

fn mapping(pairs: Vec<(&str, Value)>) -> Mapping {
    pairs
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect()
}

/// The value under `key` in the frontmatter of `document`.
fn scalar(document: &RawDocument, key: &str) -> Value {
    frontmatter(document)[key].clone()
}

fn text(text: &str) -> Value {
    Value::String(text.to_owned())
}

/// Empties the `kind` of every node of `document`.
pub fn unclaim(document: &mut RawDocument) {
    match document {
        RawDocument::Invalid(invalid) => unclaim_markdown_block(&mut invalid.body),
        RawDocument::Valid(valid) => {
            valid.kind = Kind(None);
            unclaim_markdown_block(&mut valid.body);
        }
    }
}

fn unclaim_markdown_block(block: &mut MarkdownBlock) {
    block.kind = Kind(None);
    block.sections.iter_mut().for_each(unclaim_section);
}

fn unclaim_section(section: &mut Section) {
    section.kind = Kind(None);
    if let Some(heading) = &mut section.heading {
        heading.kind = Kind(None);
        unclaim_inlines(&mut heading.children);
    }
    for block in &mut section.blocks {
        match block {
            Block::BlockQuote(block_quote) => block_quote.kind = Kind(None),
            Block::CodeBlock(code_block) => code_block.kind = Kind(None),
            Block::Html(html) => html.kind = Kind(None),
            Block::List(list) => {
                list.kind = Kind(None);
                for item in &mut list.items {
                    item.kind = Kind(None);
                    unclaim_markdown_block(&mut item.body);
                }
            }
            Block::Paragraph(paragraph) => {
                paragraph.kind = Kind(None);
                for line in &mut paragraph.lines {
                    line.kind = Kind(None);
                    unclaim_inlines(&mut line.children);
                }
            }
            Block::Table(table) => {
                table.kind = Kind(None);
                for row in table.head.iter_mut().chain(&mut table.rows) {
                    row.kind = Kind(None);
                    for cell in &mut row.cells {
                        cell.kind = Kind(None);
                        unclaim_markdown_block(&mut cell.body);
                    }
                }
            }
            Block::ThematicBreak(thematic_break) => thematic_break.kind = Kind(None),
        }
    }
    section.subsections.iter_mut().for_each(unclaim_section);
}

fn unclaim_inlines(inlines: &mut [Inline]) {
    for inline in inlines {
        match inline {
            Inline::Code(code) => code.kind = Kind(None),
            Inline::Emphasis(emphasis) => {
                emphasis.kind = Kind(None);
                unclaim_inlines(&mut emphasis.children);
            }
            Inline::Html(html) => html.kind = Kind(None),
            Inline::Image(image) => image.kind = Kind(None),
            Inline::LineBreak(line_break) => line_break.kind = Kind(None),
            Inline::Link(link) => {
                link.kind = Kind(None);
                unclaim_link_text(&mut link.children);
            }
            Inline::Strong(strong) => {
                strong.kind = Kind(None);
                unclaim_inlines(&mut strong.children);
            }
            Inline::Text(text) => text.kind = Kind(None),
        }
    }
}

fn unclaim_link_text(inlines: &mut [LinkInline]) {
    for inline in inlines {
        match inline {
            LinkInline::Code(code) => code.kind = Kind(None),
            LinkInline::Emphasis(emphasis) => {
                emphasis.kind = Kind(None);
                unclaim_link_text(&mut emphasis.children);
            }
            LinkInline::Html(html) => html.kind = Kind(None),
            LinkInline::Image(image) => image.kind = Kind(None),
            LinkInline::LineBreak(line_break) => line_break.kind = Kind(None),
            LinkInline::Strong(strong) => {
                strong.kind = Kind(None);
                unclaim_link_text(&mut strong.children);
            }
            LinkInline::Text(text) => text.kind = Kind(None),
        }
    }
}
