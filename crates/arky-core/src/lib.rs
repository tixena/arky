//! Core library for arky: markdown documents as a typed tree.

extern crate alloc;

tixschema::decode_with_foundation!();
tixschema::mongodb_foundation!();

pub mod bundle;
pub mod forms;
pub mod parser;
pub mod value;
pub mod writer;
