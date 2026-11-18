# 3. Persistence: SQLx on bundled SQLite

Date: 2026-11-18

## Context

upland is becoming an AI gateway: proxying agent APIs, serving MCP, and "and
others". Four workload classes are on the near-term table — config &
credentials (small, read-heavy), request/response logs (high write rate,
large payloads, retention matters), usage analytics (append-heavy aggregates),
and session/state (hot-path point reads and writes). The repository had no
database at all.

The wayfinder map ([#4](https://github.com/xminihub/upland/issues/4)) settled
the constraints by grilling: the **single self-contained binary** of
ADR-0001 must survive (no external database service), a later move to
Postgres must stay cheap, the data layer must be async-native (axum means
tokio everywhere), and ties break toward developer ergonomics & safety. Two
research tickets answered the facts:

- [#5](https://github.com/xminihub/upland/issues/5): SQLite carries all four
  workloads embedded-first **if** the connection shape is right — a
  multi-connection write pool holds SQLite's exclusive lock across `await`
  points (measured p99 of 182 s vs 82 ms with a single writer). The safe
  shape is one writer connection plus a separate read pool, WAL, batched log
  writes, and time-bucketed `DROP`-style pruning. Engine-change exit
  triggers: multi-node HA, sustained ≫100 req/s with full-payload retention,
  or deep interactive raw-log analytics.
- [#6](https://github.com/xminihub/upland/issues/6): among SQLx, Diesel and
  SeaORM, SQLx is the front-runner — async-native, compile-time-checked
  queries with an offline metadata workflow for CI, and the same crate API
  covering a future Postgres driver. Diesel's SQLite path is sync behind
  `spawn_blocking`, failing the async gate; SeaORM trades free SQL for an
  entity DSL that leaks SQLite's ALTER gaps.

## Decision

- **Engine: SQLite**, accessed through `sqlx` with the `sqlite-bundled`
  feature so the C library is compiled into the binary on every release
  target (ADR-0001's single-artifact story holds).
- **Data layer: SQLx 0.9, free SQL, no ORM.** Queries use the compile-time
  checked `sqlx::query!` macros; the generated `.sqlx/` metadata is
  committed and `.env` pins `SQLX_OFFLINE=true`, so builds and tests need no
  database service — the workflow CI would otherwise lack, #6 verified.
- **Migrations: `sqlx::migrate!`**, embedded from `migrations/` at compile
  time and run on the writer at startup. The baseline migration is empty of
  tables on purpose: the gateway's schema lands with the features that need
  it, each in its own migration.
- **One file now, split named as the growth path**
  ([#7](https://github.com/xminihub/upland/issues/7)): a single
  `upland.db` behind `Db`. When log volume arrives, the sanctioned move is a
  file-level split (a churned `logs.db` beside `config`/state), decided at
  that point — not today's empty files. The split also protects the Postgres
  escape: the small config file migrates with `pgloader`; the logs file
  retires.
- **Connection shape** ([#8](https://github.com/xminihub/upland/issues/8),
  [#9](https://github.com/xminihub/upland/issues/9)): `Db` owns two pools —
  a writer pool of exactly one connection and a read-only pool of four —
  because #5 measured the multi-writer shape as pathological. Per
  connection: `journal_mode=WAL`, `synchronous=NORMAL`, `busy_timeout=5s`,
  `journal_size_limit=64MiB`, `foreign_keys=ON`.
- **The database path** (CONTEXT.md): `--db-path` and `UPLAND_DB_PATH`,
  flag wins, defaulting to the platform's per-user data directory
  (`%APPDATA%` on Windows, `~/Library/Application Support` on macOS,
  `$XDG_DATA_HOME` or `~/.local/share` elsewhere). Not the working
  directory: a `brew services` unit, a systemd job and a laptop terminal
  would otherwise point at three different databases.
- **Placement**: `Db` is opened in `main` **before** the listener binds — a
  machine that cannot write its data directory fails without taking a port —
  and lives in `AppState` as `Arc<Db>`. `router(state)` takes the state;
  HTTP tests build it on an in-memory database
  (`file:upland-test-N?mode=memory&cache=shared`), keeping the repo rule
  that all behavior is tested through the in-process router seam with no
  sockets and no files in production shape.
- The database opens no endpoint of its own: `/healthz` stays the liveness
  probe it is, and readiness-with-database is a future feature's decision.

## Consequences

- `upland` grows its first runtime dependency beyond the web stack
  (`sqlx` + `libsqlite3-sys`); every release build host needs a C compiler
  for the bundled SQLite — already true of the existing toolchains.
- Adding a query or migration means running `cargo sqlx prepare` locally
  (sqlx-cli) and committing the regenerated `.sqlx/`; CI builds offline and
  fails on stale metadata, which is the drift check.
- WAL creates `-wal`/`-shm` sidecar files next to the database: backups are
  a checkpoint-then-copy of all three, and `.gitignore` never has to see
  them because the default path is outside the repository.
- sqlite is single-writer by design here: features that want more write
  concurrency hit #5's exit triggers and reopen this ADR, not tune pragmas.
- `sqlx` is pre-1.0; the lockfile pins it and every command runs `--locked`,
  so a minor bump is a deliberate, audited change to `Cargo.lock`.
- The Postgres escape stays a driver swap plus offline regeneration: the
  discipline is plain SQL in migrations and no SQLite-specific features
  outside the driver boundary.

## Rejected alternatives

- **Diesel.** The best type-safety story in the field, but its async SQLite
  support is `spawn_blocking` around a sync connection; the async-native
  requirement (#8's framing) makes it third here, not first.
- **SeaORM.** Async and convenient, but the entity DSL and its
  SemVer-breaking sqlx coupling add friction to exactly the escape hatch we
  want cheap, with no compile-time query checking to pay for it.
- **A server database (Postgres) from day one.** Contradicts ADR-0001's
  single artifact and turns `brew install upland` into a provisioning
  exercise.
- **Splitting into `logs.db`/`config.db` now.** Two of the three files would
  have no writers yet; the research split condition is log volume, which
  does not exist.
- **The working directory as the default database home.** A server that
  stores state wherever it happened to be started is a different database
  per launcher.
- **Runtime (unchecked) queries for now.** Gives up the compile-time safety
  that made SQLx the pick, and the offline metadata workflow it fears is
  three commands documented once.
