//! The `MinIO` store a `minio+http:` or `minio+https:` string names:
//! `minio+https://[key:secret@]host[:port]/bucket[/prefix][?region=…]`.

use core::future;
use std::path::PathBuf;

use os_shim::System;
use os_shim::bucket::{MinioBucket, MinioConfig};

use super::{
    Backend, Bucketed, Context, Keys, bucket_url, decoded, missing_keys, parsed, prefix_of,
};
use crate::object::{ObjectConfig, ObjectStore};
use crate::{BoxFuture, DynStore, StoreError, StorePath};

/// The options a `MinIO` store string takes.
const OPTIONS: &[&str] = &["region"];

/// The region when the string names none.
const REGION: &str = "us-east-1";

/// Where a `MinIO` store string says the store is.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Minio {
    pub bucket: String,
    pub endpoint: String,
    pub keys: Option<Keys>,
    pub prefix: Option<StorePath>,
    pub region: String,
}

/// The `MinIO` store over `minio+http` and `minio+https`.
#[derive(Debug)]
pub(super) struct MinioBackend;

impl Backend for MinioBackend {
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
        Box::pin(future::ready(
            parse(text).and_then(|minio| store(minio, context)),
        ))
    }

    fn resolve(&self, _text: &str) -> Option<Result<(String, PathBuf), StoreError>> {
        None
    }

    fn schemes(&self) -> &'static [&'static str] {
        &["minio+http", "minio+https"]
    }

    /// The bucket, `minio+https://host[:port]/bucket` with the query, and the
    /// path after it.
    fn split(&self, text: &str) -> Result<(String, String), StoreError> {
        let mut url = parsed(text)?;
        let path = url.path().trim_start_matches('/').to_owned();
        let (bucket, rest) = path.split_once('/').unwrap_or((&path, ""));
        let inside = decoded(rest)?;
        url.set_path(&format!("/{bucket}"));
        Ok((url.into(), inside))
    }
}

/// The store `minio` names, its clock `system`'s: over the keys it carries,
/// else over `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY`, a variable not
/// set being `NotConfigured`.
pub(super) fn build(
    minio: Minio,
    system: &dyn System,
) -> Result<ObjectStore<'_, MinioBucket>, StoreError> {
    if let Some(keys) = minio.keys {
        let config = MinioConfig::new(
            minio.endpoint,
            minio.region,
            minio.bucket,
            keys.access,
            keys.secret,
        );
        Ok(ObjectStore::new(
            MinioBucket::new(config),
            minio.prefix,
            system,
        ))
    } else {
        let config = ObjectConfig::new(minio.endpoint, minio.region, minio.bucket, minio.prefix);
        ObjectStore::open(system, config).map_err(missing_keys)
    }
}

/// Where the `MinIO` store string `text` says the store is.
pub(super) fn parse(text: &str) -> Result<Minio, StoreError> {
    let Bucketed {
        keys,
        mut options,
        url,
    } = bucket_url(text, OPTIONS)?;
    let Some(transport) = url.scheme().strip_prefix("minio+") else {
        return Err(StoreError::InvalidStore(
            "a MinIO store string starts with minio+http: or minio+https:".to_owned(),
        ));
    };
    let Some(host) = url.host_str().filter(|host| !host.is_empty()) else {
        return Err(StoreError::InvalidStore(
            "no host before the bucket".to_owned(),
        ));
    };
    let endpoint = url.port().map_or_else(
        || format!("{transport}://{host}"),
        |port| format!("{transport}://{host}:{port}"),
    );
    let path = url.path().trim_start_matches('/');
    let (bucket, rest) = path.split_once('/').unwrap_or((path, ""));
    if bucket.is_empty() {
        return Err(StoreError::InvalidStore("no bucket".to_owned()));
    }
    Ok(Minio {
        bucket: decoded(bucket)?,
        endpoint,
        keys,
        prefix: prefix_of(rest)?,
        region: options
            .remove("region")
            .unwrap_or_else(|| REGION.to_owned()),
    })
}

/// The store `minio` names, built by [`build`], holding the context's lock.
fn store<'store>(
    minio: Minio,
    context: Context<'store>,
) -> Result<Box<dyn DynStore + 'store>, StoreError> {
    Ok(Box::new(
        build(minio, context.system)?.locking(context.lock),
    ))
}
