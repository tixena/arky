//! A store type defined outside arky-store, over a map in memory, built and
//! locked through arky-store's public items only, and the document functions
//! over it.

extern crate alloc;

#[cfg(test)]
#[path = "outside_store/tests.rs"]
mod tests;
