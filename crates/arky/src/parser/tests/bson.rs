//! The tree written to BSON and read back with plain serde, and the typed
//! filters its types build.

use super::FIXTURES;
use super::json::unclaim;
use crate::parser::ast::RawDocument;
use crate::parser::parse;

#[test]
fn the_typed_filter_on_the_tag_asks_for_a_valid_document() {
    let valid = <RawDocument>::MONGO_FIELDS.is_valid();
    assert_eq!(
        valid.into_document().to_string(),
        r#"{ "t": { "$eq": "valid" } }"#
    );
}

#[test]
fn each_fixture_reads_back_from_bson() {
    for source in FIXTURES {
        let mut document = parse(source).document;
        let row = bson::serialize_to_document(&document).unwrap();
        let back: RawDocument = bson::deserialize_from_document(row).unwrap();
        unclaim(&mut document);
        assert_eq!(back, document);
    }
}
