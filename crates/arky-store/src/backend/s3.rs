//! The AWS S3 store an `s3:` string names, the bucket as its host as the AWS
//! command line writes it: `s3://[key:secret@]bucket[/prefix]?region=…[&kms=…]`.

use core::future;
use std::path::PathBuf;

use os_shim::bucket::{Bucket, S3Bucket, S3Config};

use super::{
    Backend, Bucketed, Context, Keys, bucket_url, decoded, missing_keys, parsed, prefix_of,
};
use crate::lock::LockSettings;
use crate::object::{ObjectStore, S3ObjectConfig};
use crate::{BoxFuture, DynStore, StoreError, StorePath};

/// The options an S3 store string takes.
const OPTIONS: &[&str] = &["kms", "region"];

/// Where an S3 store string says the store is.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct S3 {
    pub bucket: String,
    pub keys: Option<Keys>,
    pub kms: Option<String>,
    pub prefix: Option<StorePath>,
    pub region: String,
}

/// The AWS S3 store over `s3`.
#[derive(Debug)]
pub(super) struct S3Backend;

impl Backend for S3Backend {
    fn nested_options(&self) -> &'static [&'static str] {
        &[]
    }

    fn open<'call, 'store>(
        &'store self,
        text: &'call str,
        context: Context<'store>,
    ) -> BoxFuture<'call, Result<Box<dyn DynStore + 'store>, StoreError>>
    where
        'store: 'call,
    {
        Box::pin(future::ready(parse(text).and_then(|s3| store(s3, context))))
    }

    fn resolve(&self, _text: &str) -> Option<Result<(String, PathBuf), StoreError>> {
        None
    }

    fn schemes(&self) -> &'static [&'static str] {
        &["s3"]
    }

    /// The bucket, `s3://bucket` with the query, and the path after it.
    fn split(&self, text: &str) -> Result<(String, String), StoreError> {
        let mut url = parsed(text)?;
        let inside = decoded(url.path().trim_start_matches('/'))?;
        url.set_path("");
        Ok((url.into(), inside))
    }
}

/// `store` holding `lock`, and encrypting a bucket it creates with the KMS
/// key `kms` when one is given.
pub(super) fn finished<B>(
    store: ObjectStore<'_, B>,
    kms: Option<String>,
    lock: LockSettings,
) -> ObjectStore<'_, B>
where
    B: Bucket,
{
    let locked = store.locking(lock);
    if let Some(key) = kms {
        locked.encrypted_with(key)
    } else {
        locked
    }
}

/// Where the S3 store string `text` says the store is.
pub(super) fn parse(text: &str) -> Result<S3, StoreError> {
    let Bucketed {
        keys,
        mut options,
        url,
    } = bucket_url(text, OPTIONS)?;
    if url.port().is_some() {
        return Err(StoreError::InvalidStore(
            "a port, which s3 does not take: the bucket is the host".to_owned(),
        ));
    }
    let Some(bucket) = url.host_str().filter(|bucket| !bucket.is_empty()) else {
        return Err(StoreError::InvalidStore("no bucket".to_owned()));
    };
    let Some(region) = options.remove("region") else {
        return Err(StoreError::InvalidStore(
            "no region: s3 needs ?region=<region>".to_owned(),
        ));
    };
    Ok(S3 {
        bucket: decoded(bucket)?,
        keys,
        kms: options.remove("kms"),
        prefix: prefix_of(url.path())?,
        region,
    })
}

/// The store `s3` names: over the keys it carries, else over
/// `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY`, a variable not set being
/// `NotConfigured`.
fn store<'store>(
    s3: S3,
    context: Context<'store>,
) -> Result<Box<dyn DynStore + 'store>, StoreError> {
    let opened = if let Some(keys) = s3.keys {
        let config = S3Config::new(s3.region, s3.bucket, keys.access, keys.secret);
        ObjectStore::new(S3Bucket::new(config), s3.prefix, context.system)
    } else {
        let config = S3ObjectConfig::new(s3.region, s3.bucket, s3.prefix);
        ObjectStore::open_s3(context.system, config).map_err(missing_keys)?
    };
    Ok(Box::new(finished(opened, s3.kms, context.lock)))
}
