//! The binary against the AWS S3 bucket `just test-s3` names, its objects
//! under a key prefix of their own that the test removes at the end.

#[cfg(test)]
#[cfg(s3_tests)]
#[cfg(feature = "s3")]
#[path = "s3/tests.rs"]
mod tests;
