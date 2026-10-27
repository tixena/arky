//! The binary against the `RustFS` server `just test-stores` starts, its
//! objects under key prefixes of their own.

#[cfg(test)]
#[cfg(store_tests)]
#[cfg(feature = "minio")]
#[path = "minio/tests.rs"]
mod tests;
