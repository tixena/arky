//! The `MinIO` backend over the `RustFS` server `just test-stores` starts,
//! its keys in variables of the store string, under a key prefix of its own.

use std::env;
use std::path::Path;

use os_shim::mock::MemorySystem;
use uuid::Uuid;

use crate::backend::Backends;
use crate::lock::LockSettings;
use crate::{Precondition, StorePath};

#[tokio::test]
async fn a_minio_string_with_its_keys_in_variables_round_trips_a_file() {
    let endpoint = env::var("ARKY_TEST_MINIO_ENDPOINT").unwrap();
    let server = endpoint.strip_prefix("http://").unwrap();
    let bucket = env::var("ARKY_TEST_MINIO_BUCKET").unwrap();
    let system = MemorySystem::new()
        .with_env("KEY", &env::var("AWS_ACCESS_KEY_ID").unwrap())
        .unwrap()
        .with_env("SECRET", &env::var("AWS_SECRET_ACCESS_KEY").unwrap())
        .unwrap();
    let prefix = format!("backend-{}", Uuid::new_v4());
    let text = format!("minio+http://${{KEY}}:${{SECRET}}@{server}/{bucket}/{prefix}");
    let backends = Backends::default();
    let store = backends
        .open(&text, Path::new("/"), LockSettings::default(), &system)
        .await
        .unwrap();

    let path = StorePath::new("a.md").unwrap();
    store
        .write(&path, b"# A\n", &Precondition::Absent)
        .await
        .unwrap();
    assert_eq!(store.read(&path).await.unwrap().bytes, b"# A\n");
    assert_eq!(
        store.list(None).await.unwrap(),
        [StorePath::new("a.md").unwrap()]
    );
    store.delete(&path, &Precondition::None).await.unwrap();
}
