use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use sqlx::Error as SqlxError;
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};

const JOURNAL_SIZE_LIMIT: i64 = 64 * 1024 * 1024;

/// The **database** (ADR-0003): one SQLite file behind two pools. The writer
/// pool holds exactly one connection because a SQLite write transaction takes
/// the whole database's exclusive lock across `await` points — a
/// multi-connection write pool is a measured path to 100-second p99s. The
/// reader pool exists so long scans never queue behind that lock.
pub struct Db {
    writer: SqlitePool,
    reader: SqlitePool,
    path: PathBuf,
}

#[derive(Debug)]
pub enum ConnectError {
    Io(std::io::Error),
    Sqlx(SqlxError),
    Migrate(sqlx::migrate::MigrateError),
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Sqlx(error) => write!(formatter, "{error}"),
            Self::Migrate(error) => write!(formatter, "{error}"),
        }
    }
}

impl From<std::io::Error> for ConnectError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<SqlxError> for ConnectError {
    fn from(error: SqlxError) -> Self {
        Self::Sqlx(error)
    }
}

impl From<sqlx::migrate::MigrateError> for ConnectError {
    fn from(error: sqlx::migrate::MigrateError) -> Self {
        Self::Migrate(error)
    }
}

static TEST_MEMORY_NAMES: AtomicUsize = AtomicUsize::new(0);

impl Db {
    /// Open (creating if needed) the database file at `path`, run the
    /// embedded migrations on the writer, and prove the connection with one
    /// compile-time-checked query.
    pub async fn connect(path: &Path) -> Result<Self, ConnectError> {
        if let Some(parent) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let options = |read_only: bool| {
            SqliteConnectOptions::new()
                .filename(path)
                .create_if_missing(true)
                .read_only(read_only)
                .journal_mode(SqliteJournalMode::Wal)
                .synchronous(SqliteSynchronous::Normal)
                .busy_timeout(Duration::from_secs(5))
                .pragma("journal_size_limit", JOURNAL_SIZE_LIMIT.to_string())
                .foreign_keys(true)
        };
        Self::build(options, path.to_path_buf()).await
    }

    /// A shared in-memory database for tests: the same two-pool shape, the
    /// same migrations, no file and no WAL (memory journals cannot do WAL).
    /// The `file:` URI with `cache=shared` is what makes the writer pool and
    /// the reader pool see one database; `in_memory(true)` alone would hand
    /// every connection its own private `:memory:`.
    pub async fn in_memory() -> Result<Self, ConnectError> {
        let uri = format!(
            "file:upland-test-{}?mode=memory&cache=shared",
            TEST_MEMORY_NAMES.fetch_add(1, Ordering::Relaxed)
        );
        let options = |read_only: bool| {
            SqliteConnectOptions::new()
                .filename(uri.as_str())
                .read_only(read_only)
                .journal_mode(SqliteJournalMode::Memory)
                .busy_timeout(Duration::from_secs(5))
                .foreign_keys(true)
        };
        Self::build(options, PathBuf::from(&uri)).await
    }

    pub fn writer(&self) -> &SqlitePool {
        &self.writer
    }

    pub fn reader(&self) -> &SqlitePool {
        &self.reader
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub async fn close(&self) {
        self.writer.close().await;
        self.reader.close().await;
    }

    async fn build<F>(options: F, reported_path: PathBuf) -> Result<Self, ConnectError>
    where
        F: Fn(bool) -> SqliteConnectOptions,
    {
        let writer = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options(false))
            .await?;
        sqlx::migrate!("./migrations").run(&writer).await?;
        // The pattern every future query in upland follows: checked at
        // compile time against the schema in `migrations/`, offline through
        // the committed `.sqlx` metadata (ADR-0003). It doubles as the
        // post-migration liveness ping.
        sqlx::query!("SELECT 1 AS ok").fetch_one(&writer).await?;

        let reader = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options(true))
            .await?;

        Ok(Self {
            writer,
            reader,
            path: reported_path,
        })
    }
}
