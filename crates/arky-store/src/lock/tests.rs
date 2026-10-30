//! Taking, checking and giving back a lock over a store whose answers are
//! scripted; then each case of the lock over the file store on
//! `MemorySystem` and, with the `minio` feature, over the `MinIO` store on
//! `MemoryBucket`. The clock is `MemorySystem`'s in every test.

use alloc::collections::{BTreeMap, VecDeque};
use core::future;
use core::iter;
use core::time::Duration;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};
use std::time::SystemTime;

#[cfg(feature = "minio")]
use os_shim::bucket::MemoryBucket;
use os_shim::mock::MemorySystem;
use os_shim::{System, WriteMode};
use uuid::Uuid;

use super::{
    Found, Held, LockSettings, Locks, SUFFIX, check, expired, finish, lock_of, remove_expired, take,
};
use crate::file::FileStore;
#[cfg(feature = "minio")]
use crate::object::ObjectStore;
use crate::{Precondition, Revision, Store, StoreError, StorePath, write_at_paths};

/// Runs `$scenario` over the file store on a clock of its own and, with the
/// `minio` feature, over the `MinIO` store on another.
macro_rules! on_each_store {
    ($scenario:ident) => {{
        let on_disk = clock();
        $scenario(&FileStore::new(PathBuf::from("/vault"), &on_disk), &on_disk).await;
        #[cfg(feature = "minio")]
        {
            let in_bucket = clock();
            let objects =
                ObjectStore::new(MemoryBucket::default(), Some(path("vault")), &in_bucket);
            $scenario(&objects, &in_bucket).await;
        }
    }};
}

const A_PNG: &[u8] = b"\x89PNG\r\n\x1a\n the picture of save A";

const B_PNG: &[u8] = b"\x89PNG\r\n\x1a\n the picture of save B";

/// The time every clock starts at, in seconds since 1970-01-01 UTC.
const START: u64 = 1_792_700_000;

const TEXT: &str = "# Report\n\n![Logo](media/logo.png)\n";

/// What `FileStore` and `ObjectStore` both do for `arky fix`.
trait Repairs {
    fn remove_expired(
        &self,
        under: Option<&StorePath>,
    ) -> impl Future<Output = Result<Vec<StorePath>, StoreError>> + Send;
}

impl Repairs for FileStore<'_> {
    fn remove_expired(
        &self,
        under: Option<&StorePath>,
    ) -> impl Future<Output = Result<Vec<StorePath>, StoreError>> + Send {
        self.remove_expired_locks(under)
    }
}

#[cfg(feature = "minio")]
impl Repairs for ObjectStore<'_, MemoryBucket> {
    fn remove_expired(
        &self,
        under: Option<&StorePath>,
    ) -> impl Future<Output = Result<Vec<StorePath>, StoreError>> + Send {
        self.remove_expired_locks(under)
    }
}

/// A store whose answers on a lock are scripted, one queue for each
/// operation, and which records each call.
struct Script {
    calls: Mutex<Vec<String>>,
    creates: Mutex<VecDeque<Result<bool, StoreError>>>,
    finds: Mutex<VecDeque<Result<Option<Found>, StoreError>>>,
    removes: Mutex<VecDeque<Result<bool, StoreError>>>,
    system: MemorySystem,
}

impl Script {
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn new(
        creates: Vec<Result<bool, StoreError>>,
        finds: Vec<Result<Option<Found>, StoreError>>,
        removes: Vec<Result<bool, StoreError>>,
    ) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            creates: Mutex::new(creates.into()),
            finds: Mutex::new(finds.into()),
            removes: Mutex::new(removes.into()),
            system: clock(),
        }
    }

    /// Records `call`, and gives the next of `answers`.
    fn next<T>(
        &self,
        call: String,
        answers: &Mutex<VecDeque<Result<T, StoreError>>>,
    ) -> Result<T, StoreError> {
        let unscripted = StoreError::Backend(format!("no answer scripted for {call}"));
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(call);
        answers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front()
            .unwrap_or(Err(unscripted))
    }
}

impl Locks for Script {
    fn create(
        &self,
        _lock: &StorePath,
        _guid: &str,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        future::ready(self.next("create".to_owned(), &self.creates))
    }

    fn find(
        &self,
        _lock: &StorePath,
    ) -> impl Future<Output = Result<Option<Found>, StoreError>> + Send {
        future::ready(self.next("find".to_owned(), &self.finds))
    }

    fn remove_if(
        &self,
        _lock: &StorePath,
        found: &Found,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        let call = format!("remove_if {}", String::from_utf8_lossy(&found.tag));
        future::ready(self.next(call, &self.removes))
    }

    fn revision(
        &self,
        _path: &StorePath,
    ) -> impl Future<Output = Result<Option<Revision>, StoreError>> + Send {
        future::ready(Err(StoreError::Backend(
            "no answer scripted for revision".to_owned(),
        )))
    }

    fn system(&self) -> &dyn System {
        &self.system
    }
}

/// A clock at `START`.
fn clock() -> MemorySystem {
    MemorySystem::new().with_now(started()).unwrap()
}

fn down() -> StoreError {
    StoreError::Backend("notes/report.md.arky-lock: no answer".to_owned())
}

/// The GUID the lock of `notes/report.md` holds in `store`, or `None` when
/// it has no lock.
async fn holder_of<S>(store: &S) -> Option<String>
where
    S: Locks + Sync,
{
    let lock = lock_of(&report()).unwrap();
    store.find(&lock).await.unwrap().map(|found| found.holder)
}

/// A lock that `holder` took `ago` before `START`.
fn lock_by(holder: &str, ago: Duration) -> Found {
    Found {
        holder: holder.to_owned(),
        tag: holder.as_bytes().to_vec(),
        taken: started() - ago,
    }
}

/// Makes the lock of `notes/report.md` in `store`, held by `holder`.
async fn lock_held_by<S>(store: &S, holder: &str)
where
    S: Locks + Sync,
{
    let lock = lock_of(&report()).unwrap();
    assert!(store.create(&lock, holder).await.unwrap());
}

/// `notes/media/logo.png` holding `bytes`.
fn logo(bytes: &[u8]) -> BTreeMap<StorePath, Vec<u8>> {
    BTreeMap::from([(path("notes/media/logo.png"), bytes.to_vec())])
}

fn mine() -> Held {
    Held {
        guid: "mine".to_owned(),
        lock: path("notes/report.md.arky-lock"),
    }
}

fn names(paths: &[StorePath]) -> Vec<&str> {
    paths.iter().map(StorePath::as_str).collect()
}

fn path(text: &str) -> StorePath {
    StorePath::new(text).unwrap()
}

fn report() -> StorePath {
    path("notes/report.md")
}

/// A save of `notes/report.md` embedding `bytes` as its logo.
async fn save<S>(
    store: &S,
    text: &str,
    bytes: &[u8],
    precondition: &Precondition,
) -> Result<Revision, StoreError>
where
    S: Store + Sync,
{
    store
        .write_with_embedded(&report(), text, &logo(bytes), precondition)
        .await
}

/// How far the clock of `system` is past `START`.
fn since_start(system: &dyn System) -> Duration {
    system.now().duration_since(started()).unwrap()
}

fn started() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(START)
}

#[test]
fn a_lock_lasts_5_seconds_a_save_waits_2_and_the_lock_of_a_file_is_beside_it() {
    let settings = LockSettings::new(Duration::from_secs(5), Duration::from_secs(2));
    assert_eq!(LockSettings::default(), settings);
    assert_eq!(SUFFIX, ".arky-lock");
    let lock = lock_of(&report()).unwrap();
    assert_eq!(lock.as_str(), "notes/report.md.arky-lock");
}

#[test]
fn a_lock_has_expired_only_once_it_is_older_than_the_settings_say() {
    let settings = LockSettings::default();
    let at = |ago| lock_by("other", ago);
    assert!(!expired(&at(Duration::ZERO), started(), settings));
    assert!(!expired(&at(Duration::from_secs(5)), started(), settings));
    assert!(expired(
        &at(Duration::from_millis(5_001)),
        started(),
        settings
    ));
    let ahead = Found {
        holder: "other".to_owned(),
        tag: Vec::new(),
        taken: started() + Duration::from_secs(60),
    };
    assert!(!expired(&ahead, started(), settings));
}

#[tokio::test]
async fn a_lock_refused_then_gone_is_taken_on_the_next_pass() {
    let script = Script::new(vec![Ok(false), Ok(true)], vec![Ok(None)], Vec::new());
    let held = take(&script, &report(), LockSettings::default())
        .await
        .unwrap();
    assert_eq!(held.lock.as_str(), "notes/report.md.arky-lock");
    assert_eq!(Uuid::parse_str(&held.guid).unwrap().get_version_num(), 4);
    assert_eq!(held.guid.len(), 36);
    assert_eq!(script.calls(), ["create", "find", "create"]);
    assert_eq!(since_start(&script.system), Duration::ZERO);
}

#[tokio::test]
async fn a_lock_that_has_not_expired_is_waited_on_once_and_still_there_is_a_conflict() {
    let fresh = || Ok(Some(lock_by("other", Duration::ZERO)));
    let script = Script::new(
        vec![Ok(false), Ok(false)],
        vec![fresh(), fresh()],
        Vec::new(),
    );
    let taken = take(&script, &report(), LockSettings::default()).await;
    assert_eq!(taken.err(), Some(StoreError::Conflict(report())));
    assert_eq!(script.calls(), ["create", "find", "create", "find"]);
    assert_eq!(since_start(&script.system), Duration::from_secs(2));
}

#[tokio::test]
async fn a_lock_gone_after_the_wait_is_taken() {
    let fresh = Ok(Some(lock_by("other", Duration::ZERO)));
    let script = Script::new(vec![Ok(false), Ok(true)], vec![fresh], Vec::new());
    take(&script, &report(), LockSettings::default())
        .await
        .unwrap();
    assert_eq!(script.calls(), ["create", "find", "create"]);
    assert_eq!(since_start(&script.system), Duration::from_secs(2));
}

#[tokio::test]
async fn an_expired_lock_is_removed_while_it_is_the_one_found_and_the_pass_starts_over() {
    let old = Ok(Some(lock_by("other", Duration::from_secs(6))));
    let script = Script::new(vec![Ok(false), Ok(true)], vec![old], vec![Ok(true)]);
    take(&script, &report(), LockSettings::default())
        .await
        .unwrap();
    assert_eq!(
        script.calls(),
        ["create", "find", "remove_if other", "create"]
    );
    assert_eq!(since_start(&script.system), Duration::ZERO);
}

#[tokio::test]
async fn a_remove_that_finds_another_lock_starts_the_pass_over_on_that_lock() {
    let old = Ok(Some(lock_by("other", Duration::from_secs(6))));
    let fresh = Ok(Some(lock_by("third", Duration::ZERO)));
    let creates = vec![Ok(false), Ok(false), Ok(true)];
    let script = Script::new(creates, vec![old, fresh], vec![Ok(false)]);
    take(&script, &report(), LockSettings::default())
        .await
        .unwrap();
    assert_eq!(
        script.calls(),
        [
            "create",
            "find",
            "remove_if other",
            "create",
            "find",
            "create"
        ]
    );
    assert_eq!(since_start(&script.system), Duration::from_secs(2));
}

#[tokio::test]
async fn a_lock_that_keeps_coming_back_expired_is_a_conflict_after_10_passes() {
    let old = || Ok(Some(lock_by("other", Duration::from_secs(6))));
    let script = Script::new(
        vec![Ok(false); 10],
        iter::repeat_with(old).take(10).collect(),
        vec![Ok(true); 10],
    );
    let taken = take(&script, &report(), LockSettings::default()).await;
    assert_eq!(taken.err(), Some(StoreError::Conflict(report())));
    let calls = script.calls();
    assert_eq!(calls.len(), 30);
    assert_eq!(calls.iter().filter(|call| *call == "create").count(), 10);
}

#[tokio::test]
async fn a_failure_of_the_store_stops_the_take() {
    let script = Script::new(vec![Err(down())], Vec::new(), Vec::new());
    let taken = take(&script, &report(), LockSettings::default()).await;
    assert_eq!(taken.err(), Some(down()));
}

#[tokio::test]
async fn a_check_passes_only_while_the_lock_holds_this_saves_guid_and_has_not_expired() {
    let finds = vec![
        Ok(Some(lock_by("mine", Duration::from_secs(5)))),
        Ok(Some(lock_by("other", Duration::ZERO))),
        Ok(Some(lock_by("mine", Duration::from_secs(6)))),
        Ok(None),
        Err(down()),
    ];
    let script = Script::new(Vec::new(), finds, Vec::new());
    let settings = LockSettings::default();
    let conflict = Err(StoreError::Conflict(report()));
    assert_eq!(check(&script, &mine(), &report(), settings).await, Ok(()));
    for expected in [conflict.clone(), conflict.clone(), conflict, Err(down())] {
        assert_eq!(check(&script, &mine(), &report(), settings).await, expected);
    }
}

#[tokio::test]
async fn a_save_gives_back_its_own_lock_and_leaves_another_saves() {
    let revision = Ok(Revision("sha256:00".to_owned()));
    let failed = Err(StoreError::Conflict(report()));
    let mine_found = || Some(lock_by("mine", Duration::ZERO));
    let cases = [
        (mine_found(), Ok(true), revision.clone(), "remove_if mine"),
        (mine_found(), Ok(false), revision.clone(), "remove_if mine"),
        (mine_found(), Ok(true), failed, "remove_if mine"),
        (
            Some(lock_by("other", Duration::ZERO)),
            Ok(true),
            revision.clone(),
            "",
        ),
        (None, Ok(true), revision, ""),
    ];
    for (found, removed, saved, remove) in cases {
        let script = Script::new(Vec::new(), vec![Ok(found)], vec![removed]);
        let given = finish(&script, &mine(), &report(), saved.clone()).await;
        assert_eq!(given, saved);
        let calls = script.calls();
        assert_eq!(calls.first().map(String::as_str), Some("find"));
        assert_eq!(calls.get(1).map_or("", String::as_str), remove);
    }
}

#[tokio::test]
async fn a_lock_not_given_back_after_a_stored_document_says_the_document_is_stored() {
    let script = Script::new(Vec::new(), vec![Err(down())], Vec::new());
    let saved = Ok(Revision("sha256:00".to_owned()));
    let given = finish(&script, &mine(), &report(), saved).await;
    let message = "notes/report.md: the document is stored, and its lock was not given back: notes/report.md.arky-lock: no answer; it expires by itself";
    assert_eq!(given, Err(StoreError::Backend(message.to_owned())));
}

#[tokio::test]
async fn a_lock_not_given_back_after_a_failed_save_adds_to_the_saves_message() {
    let mine_found = Ok(Some(lock_by("mine", Duration::ZERO)));
    let script = Script::new(Vec::new(), vec![mine_found], vec![Err(down())]);
    let saved = Err(StoreError::Conflict(report()));
    let given = finish(&script, &mine(), &report(), saved).await;
    let message = "notes/report.md: the file changed, or a file is already there; its lock was not given back: notes/report.md.arky-lock: no answer";
    assert_eq!(given, Err(StoreError::Backend(message.to_owned())));
}

async fn one_save<S>(store: &S, system: &MemorySystem)
where
    S: Store + Locks + Sync,
{
    save(store, TEXT, A_PNG, &Precondition::None).await.unwrap();
    assert_eq!(store.read(&report()).await.unwrap().bytes, TEXT.as_bytes());
    let logo_path = path("notes/media/logo.png");
    assert_eq!(store.read(&logo_path).await.unwrap().bytes, A_PNG);
    assert_eq!(holder_of(store).await, None);
    assert_eq!(since_start(system), Duration::ZERO);
}

#[tokio::test]
async fn a_save_writes_its_files_and_gives_its_lock_back() {
    on_each_store!(one_save);
}

async fn held_by_another<S>(store: &S, system: &MemorySystem)
where
    S: Store + Locks + Sync,
{
    lock_held_by(store, "another").await;
    let saved = save(store, TEXT, A_PNG, &Precondition::None).await;
    assert_eq!(saved, Err(StoreError::Conflict(report())));
    assert_eq!(since_start(system), Duration::from_secs(2));
    assert_eq!(store.list(None).await.unwrap(), Vec::<StorePath>::new());
    assert_eq!(holder_of(store).await.as_deref(), Some("another"));
}

#[tokio::test]
async fn a_save_waits_2_seconds_on_a_lock_that_has_not_expired_then_is_a_conflict_that_wrote_nothing()
 {
    on_each_store!(held_by_another);
}

async fn expired_lock<S>(store: &S, system: &MemorySystem)
where
    S: Store + Locks + Sync,
{
    lock_held_by(store, "another").await;
    system.advance(Duration::from_secs(6)).unwrap();
    save(store, TEXT, A_PNG, &Precondition::None).await.unwrap();
    assert_eq!(since_start(system), Duration::from_secs(6));
    assert_eq!(holder_of(store).await, None);
    let listed = store.list(None).await.unwrap();
    assert_eq!(names(&listed), ["notes/media/logo.png", "notes/report.md"]);
}

#[tokio::test]
async fn a_save_removes_an_expired_lock_takes_its_own_and_saves() {
    on_each_store!(expired_lock);
}

async fn two_saves<S>(store: &S, system: &MemorySystem)
where
    S: Store + Locks + Sync,
{
    let settings = LockSettings::default();
    let document = report();
    let logo_path = path("notes/media/logo.png");
    let held = take(store, &report(), settings).await.unwrap();
    check(store, &held, &report(), settings).await.unwrap();
    store
        .write(&logo_path, A_PNG, &Precondition::None)
        .await
        .unwrap();

    let other = "# Other\n\n![Logo](media/logo.png)\n";
    let rejected = save(store, other, B_PNG, &Precondition::None).await;
    assert_eq!(rejected, Err(StoreError::Conflict(report())));
    assert_eq!(since_start(system), Duration::from_secs(2));

    let saved = write_at_paths(
        store,
        &report(),
        TEXT,
        &BTreeMap::new(),
        &Precondition::None,
        || check(store, &held, &document, settings),
    )
    .await;
    finish(store, &held, &report(), saved).await.unwrap();
    assert_eq!(store.read(&report()).await.unwrap().bytes, TEXT.as_bytes());
    assert_eq!(store.read(&logo_path).await.unwrap().bytes, A_PNG);
    assert_eq!(holder_of(store).await, None);
}

#[tokio::test]
async fn of_two_saves_of_one_document_the_one_that_finds_the_lock_writes_nothing() {
    on_each_store!(two_saves);
}

async fn lost_mid_save<S>(store: &S, system: &MemorySystem)
where
    S: Store + Locks + Sync,
{
    let settings = LockSettings::default();
    let document = report();
    let logo_path = path("notes/media/logo.png");
    let chart = path("notes/media/chart.png");
    let held = take(store, &report(), settings).await.unwrap();
    check(store, &held, &report(), settings).await.unwrap();
    store
        .write(&logo_path, A_PNG, &Precondition::None)
        .await
        .unwrap();

    system.advance(Duration::from_secs(6)).unwrap();
    let other = take(store, &report(), settings).await.unwrap();

    let files = BTreeMap::from([(chart.clone(), B_PNG.to_vec())]);
    let saved = write_at_paths(store, &report(), TEXT, &files, &Precondition::None, || {
        check(store, &held, &document, settings)
    })
    .await;
    let given = finish(store, &held, &report(), saved).await;
    assert_eq!(given, Err(StoreError::Conflict(report())));
    assert_eq!(store.read(&logo_path).await.unwrap().bytes, A_PNG);
    assert_eq!(store.read(&chart).await, Err(StoreError::NotFound(chart)));
    assert_eq!(
        store.read(&report()).await,
        Err(StoreError::NotFound(report()))
    );
    assert_eq!(holder_of(store).await, Some(other.guid));
}

#[tokio::test]
async fn a_save_whose_lock_another_save_took_over_stops_before_its_next_file() {
    on_each_store!(lost_mid_save);
}

async fn second_remover<S>(store: &S, system: &MemorySystem)
where
    S: Locks + Sync,
{
    let lock = lock_of(&report()).unwrap();
    lock_held_by(store, "another").await;
    system.advance(Duration::from_secs(6)).unwrap();
    let first = store.find(&lock).await.unwrap().unwrap();
    let second = store.find(&lock).await.unwrap().unwrap();
    assert!(store.remove_if(&lock, &first).await.unwrap());
    assert!(store.create(&lock, "first").await.unwrap());
    assert!(!store.remove_if(&lock, &second).await.unwrap());
    assert_eq!(holder_of(store).await.as_deref(), Some("first"));
}

#[tokio::test]
async fn of_two_removers_of_one_expired_lock_the_second_leaves_the_lock_the_first_took() {
    on_each_store!(second_remover);
}

async fn listing<S>(store: &S, _system: &MemorySystem)
where
    S: Store + Locks + Sync,
{
    save(store, TEXT, A_PNG, &Precondition::None).await.unwrap();
    lock_held_by(store, "another").await;
    let every = ["notes/media/logo.png", "notes/report.md"];
    assert_eq!(names(&store.list(None).await.unwrap()), every);
    let notes = store.list(Some(&path("notes"))).await.unwrap();
    assert_eq!(names(&notes), every);
}

#[tokio::test]
async fn a_listing_leaves_locks_out() {
    on_each_store!(listing);
}

async fn plain_writes<S>(store: &S, system: &MemorySystem)
where
    S: Store + Locks + Sync,
{
    lock_held_by(store, "another").await;
    store
        .write(&report(), TEXT.as_bytes(), &Precondition::None)
        .await
        .unwrap();
    store.delete(&report(), &Precondition::None).await.unwrap();
    assert_eq!(holder_of(store).await.as_deref(), Some("another"));
    assert_eq!(since_start(system), Duration::ZERO);
}

#[tokio::test]
async fn a_write_and_a_delete_of_one_file_take_no_lock() {
    on_each_store!(plain_writes);
}

async fn stale_revision<S>(store: &S, _system: &MemorySystem)
where
    S: Store + Locks + Sync,
{
    let first = save(store, TEXT, A_PNG, &Precondition::None).await.unwrap();
    let second = "# Report, revised\n\n![Logo](media/logo.png)\n";
    save(store, second, A_PNG, &Precondition::None)
        .await
        .unwrap();
    let stale = Precondition::Matches(first);
    let saved = save(store, "# Late\n", B_PNG, &stale).await;
    assert_eq!(saved, Err(StoreError::Conflict(report())));
    assert_eq!(
        store.read(&report()).await.unwrap().bytes,
        second.as_bytes()
    );
    assert_eq!(logo_bytes(store).await, A_PNG);
    assert_eq!(holder_of(store).await, None);
}

#[tokio::test]
async fn a_save_under_a_stale_revision_is_a_conflict_checked_under_the_lock() {
    on_each_store!(stale_revision);
}

async fn logo_bytes<S>(store: &S) -> Vec<u8>
where
    S: Store + Sync,
{
    store
        .read(&path("notes/media/logo.png"))
        .await
        .unwrap()
        .bytes
}

async fn absent_with_a_document<S>(store: &S, _system: &MemorySystem)
where
    S: Store + Locks + Sync,
{
    save(store, TEXT, A_PNG, &Precondition::None).await.unwrap();
    let saved = save(store, "# Late\n", B_PNG, &Precondition::Absent).await;
    assert_eq!(saved, Err(StoreError::Conflict(report())));
    assert_eq!(store.read(&report()).await.unwrap().bytes, TEXT.as_bytes());
    assert_eq!(logo_bytes(store).await, A_PNG);
    assert_eq!(holder_of(store).await, None);
}

#[tokio::test]
async fn a_save_under_absent_over_a_document_is_a_conflict_that_writes_no_picture() {
    on_each_store!(absent_with_a_document);
}

async fn current_revision<S>(store: &S, _system: &MemorySystem)
where
    S: Store + Locks + Sync,
{
    let first = save(store, TEXT, A_PNG, &Precondition::None).await.unwrap();
    let next = "# Report, revised\n\n![Logo](media/logo.png)\n";
    let saved = save(store, next, B_PNG, &Precondition::Matches(first)).await;
    assert_eq!(
        saved.unwrap(),
        store.read(&report()).await.unwrap().revision
    );
    assert_eq!(store.read(&report()).await.unwrap().bytes, next.as_bytes());
    assert_eq!(logo_bytes(store).await, B_PNG);
    assert_eq!(holder_of(store).await, None);
}

#[tokio::test]
async fn a_save_under_the_current_revision_writes_its_pictures_and_the_document() {
    on_each_store!(current_revision);
}

async fn own_settings<S>(store: &S, system: &MemorySystem)
where
    S: Store + Locks + Sync,
{
    lock_held_by(store, "another").await;
    system.advance(Duration::from_secs(6)).unwrap();
    let saved = save(store, TEXT, A_PNG, &Precondition::None).await;
    assert_eq!(saved, Err(StoreError::Conflict(report())));
    assert_eq!(since_start(system), Duration::from_secs(9));
    system.advance(Duration::from_secs(2)).unwrap();
    save(store, TEXT, A_PNG, &Precondition::None).await.unwrap();
    assert_eq!(holder_of(store).await, None);
}

#[tokio::test]
async fn a_store_waits_and_expires_locks_as_its_settings_say() {
    let settings = LockSettings::new(Duration::from_secs(10), Duration::from_secs(3));
    let on_disk = clock();
    let files = FileStore::new(PathBuf::from("/vault"), &on_disk).locking(settings);
    own_settings(&files, &on_disk).await;
    #[cfg(feature = "minio")]
    {
        let in_bucket = clock();
        let objects = ObjectStore::new(MemoryBucket::default(), None, &in_bucket).locking(settings);
        own_settings(&objects, &in_bucket).await;
    }
}

#[tokio::test]
async fn the_sample_waits_on_a_lock_file_then_removes_it_once_it_has_expired() {
    let system = MemorySystem::new().with_dir("/vault/notes").unwrap();
    let lock = "/vault/notes/report.md.arky-lock".as_ref();
    let guid = "a41c1f02-77e4-4b8e-9d0c-3e5b8f6a2d19";
    system
        .write(lock, guid.as_bytes(), WriteMode::CreateNew)
        .unwrap();
    let store = FileStore::new("/vault".into(), &system);
    let saved = save(&store, TEXT, A_PNG, &Precondition::None).await;
    assert!(matches!(saved, Err(StoreError::Conflict(_))), "{saved:?}");
    assert!(
        !system
            .exists("/vault/notes/media/logo.png".as_ref())
            .unwrap()
    );
    system.advance(Duration::from_secs(4)).unwrap();
    save(&store, TEXT, A_PNG, &Precondition::None)
        .await
        .unwrap();
    assert!(!system.exists(lock).unwrap());
}

async fn make_lock<S>(store: &S, file: &str)
where
    S: Locks + Sync,
{
    assert!(
        store
            .create(&lock_of(&path(file)).unwrap(), "another")
            .await
            .unwrap()
    );
}

async fn removes_what_expired<S>(store: &S, system: &MemorySystem)
where
    S: Store + Locks + Repairs + Sync,
{
    make_lock(store, "notes/parts/b.md").await;
    make_lock(store, "notes/report.md").await;
    store
        .write(&path("notes/Cargo.lock"), b"old", &Precondition::None)
        .await
        .unwrap();
    system.advance(Duration::from_secs(6)).unwrap();
    make_lock(store, "notes/fresh.md").await;
    let removed = store.remove_expired(None).await.unwrap();
    assert_eq!(
        names(&removed),
        ["notes/parts/b.md.arky-lock", "notes/report.md.arky-lock"]
    );
    assert_eq!(holder_of(store).await, None);
    let fresh = lock_of(&path("notes/fresh.md")).unwrap();
    assert!(store.find(&fresh).await.unwrap().is_some());
    assert_eq!(
        store.read(&path("notes/Cargo.lock")).await.unwrap().bytes,
        b"old"
    );
    assert_eq!(
        store.remove_expired(None).await.unwrap(),
        Vec::<StorePath>::new()
    );
}

#[tokio::test]
async fn removing_expired_locks_removes_each_one_that_expired_and_leaves_the_rest() {
    on_each_store!(removes_what_expired);
}

async fn removes_under_a_folder<S>(store: &S, system: &MemorySystem)
where
    S: Store + Locks + Repairs + Sync,
{
    make_lock(store, "notes/report.md").await;
    make_lock(store, "notes/parts/b.md").await;
    make_lock(store, "other/c.md").await;
    system.advance(Duration::from_secs(6)).unwrap();
    let removed = store.remove_expired(Some(&path("notes"))).await.unwrap();
    assert_eq!(
        names(&removed),
        ["notes/parts/b.md.arky-lock", "notes/report.md.arky-lock"]
    );
    let other = lock_of(&path("other/c.md")).unwrap();
    assert!(store.find(&other).await.unwrap().is_some());
}

#[tokio::test]
async fn removing_expired_locks_under_a_folder_leaves_the_locks_outside_it() {
    on_each_store!(removes_under_a_folder);
}

#[tokio::test]
async fn an_expired_lock_replaced_after_it_was_found_is_left_and_not_reported() {
    let old = Ok(Some(lock_by("other", Duration::from_secs(6))));
    let script = Script::new(Vec::new(), vec![old], vec![Ok(false)]);
    let removed = remove_expired(&script, &[report()], LockSettings::default())
        .await
        .unwrap();
    assert_eq!(removed, Vec::<StorePath>::new());
    assert_eq!(script.calls(), ["find", "remove_if other"]);
}

#[tokio::test]
async fn a_lock_gone_before_it_was_found_is_skipped() {
    let script = Script::new(Vec::new(), vec![Ok(None)], Vec::new());
    let removed = remove_expired(&script, &[report()], LockSettings::default())
        .await
        .unwrap();
    assert_eq!(removed, Vec::<StorePath>::new());
    assert_eq!(script.calls(), ["find"]);
}

#[tokio::test]
async fn expires_decides_which_locks_have_expired() {
    let ten_seconds = || Ok(Some(lock_by("other", Duration::from_secs(10))));
    let script = Script::new(Vec::new(), vec![ten_seconds()], Vec::new());
    let minute = LockSettings::new(Duration::from_secs(60), Duration::from_secs(2));
    let removed = remove_expired(&script, &[report()], minute).await.unwrap();
    assert_eq!(removed, Vec::<StorePath>::new());
    assert_eq!(script.calls(), ["find"]);
    let expiring = Script::new(Vec::new(), vec![ten_seconds()], vec![Ok(true)]);
    let expired_at_five = remove_expired(&expiring, &[report()], LockSettings::default())
        .await
        .unwrap();
    assert_eq!(expired_at_five, [report()]);
}

#[tokio::test]
async fn a_failure_of_the_store_stops_the_removal() {
    let script = Script::new(Vec::new(), vec![Err(down())], Vec::new());
    let removed = remove_expired(&script, &[report()], LockSettings::default()).await;
    assert_eq!(removed, Err(down()));
}
