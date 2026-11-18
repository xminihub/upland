use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

struct TempDb(PathBuf);

impl TempDb {
    fn new(name: &str) -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "upland-db-test-{}-{unique}-{name}.db",
            std::process::id()
        ));
        Self(path)
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let mut path = self.0.clone().into_os_string();
            path.push(suffix);
            let _ = std::fs::remove_file(PathBuf::from(&path));
        }
    }
}

#[tokio::test]
async fn the_baseline_migration_is_recorded() {
    let db = upland::db::Db::in_memory().await.expect("db");
    let (version,): (i64,) = sqlx::query_as("SELECT MAX(version) FROM _sqlx_migrations")
        .fetch_one(db.reader())
        .await
        .expect("migration history");
    assert_eq!(version, 20261118000002);
    db.close().await;
}

#[tokio::test]
async fn in_memory_databases_are_shared_between_the_pools() {
    // The two-pool shape is what every HTTP test runs on; if an in-memory
    // name were not shared, tests would pass for the handlers and fail for
    // anything that reads, so the shape is asserted directly.
    let db = upland::db::Db::in_memory().await.expect("db");
    sqlx::raw_sql(
        "CREATE TABLE shared_probe (n INTEGER); INSERT INTO shared_probe (n) VALUES (0);",
    )
    .execute(db.writer())
    .await
    .expect("create on the writer");
    let (n,): (i64,) = sqlx::query_as("SELECT n FROM shared_probe")
        .fetch_one(db.reader())
        .await
        .expect("read on the reader");
    assert_eq!(n, 0);
    db.close().await;
}

#[tokio::test]
async fn writer_and_reader_share_one_file_database() {
    let temp = TempDb::new("shared");
    let db = upland::db::Db::connect(&temp.0).await.expect("db");

    sqlx::raw_sql("CREATE TABLE pingpong (value INTEGER NOT NULL)")
        .execute(db.writer())
        .await
        .expect("create");
    sqlx::raw_sql("INSERT INTO pingpong (value) VALUES (1), (2)")
        .execute(db.writer())
        .await
        .expect("insert");

    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM pingpong")
        .fetch_one(db.reader())
        .await
        .expect("read through the reader pool");
    assert_eq!(count, 2);
    db.close().await;
}

#[tokio::test]
async fn file_databases_run_in_wal_mode() {
    // WAL is the condition the concurrency model rests on (ADR-0003); an
    // accidental rollback to the delete journal shows up here, not in
    // production p99s.
    let temp = TempDb::new("wal");
    let db = upland::db::Db::connect(&temp.0).await.expect("db");
    let (mode,): (String,) = sqlx::query_as("PRAGMA journal_mode")
        .fetch_one(db.reader())
        .await
        .expect("pragma");
    assert_eq!(mode.to_lowercase(), "wal");
}

#[tokio::test]
async fn connect_creates_a_missing_parent_directory() {
    let temp = TempDb::new("nested");
    let path = temp.0.parent().unwrap().join("nested").join("upland.db");
    let db = upland::db::Db::connect(&path).await.expect("db");
    assert_eq!(db.path(), path);
    assert!(path.exists());
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir(path.parent().unwrap());
}
