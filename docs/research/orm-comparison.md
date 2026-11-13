# Research: SQLx vs Diesel vs SeaORM under upland's constraints

- **Issue:** xminihub/upland#6
- **Date:** 2026-11-13
- **Status:** research only — no decision made here; feeds the downstream decision ticket.

## Scope and constraints

Upland is an axum (tokio) app shipped as a **single self-contained release binary**
(linux-amd64, macos-arm64, windows-amd64; SPA embedded via `rust-embed`). Persistence is
**embedded-first (SQLite expected)** for an AI-gateway workload: config/credentials,
request/response logs, usage analytics, session/state.

Hard constraints (settled by the developer):

1. **Async-native (tokio) is mandatory.**
2. **Embedded-first single binary must survive** — favor bundled/static builds.
3. **A later migration to Postgres must stay cheap** — avoid dialect lock-in.
4. Tie-breaks: **developer ergonomics & safety.**

## Ecosystem snapshot (as of 2026-11-13)

| Crate | Latest stable | Released | Signals |
|---|---|---|---|
| `sqlx` | 0.9.0 | 2026-05-21 (crates.io) | Repo moved to **transact-rs** org in the 0.9.0 cycle; 17.7k stars, pushed 2026-11-10; MSRV 1.94 |
| `diesel` | 2.4.0 | **2026-11-13 (today)** | 14.2k stars; 2.3.x line still patched (2.3.14 on 2026-10-07); pushed today |
| `diesel-async` | 0.10.0 | **2026-11-13 (today)** | Pinned to `diesel ~2.4.0`; 0.9.0 was 2026-04-30 |
| `sea-orm` | 2.0.4 | 2026-09-27 (2.0.0 was 2026-07-19) | 10.0k stars, pushed 2026-11-10; 250k+ weekly downloads (project homepage) |
| `libsqlite3-sys` | 0.40.0 | 2026-11-11 | Shared SQLite binding under sqlx / rusqlite / Diesel's `bundled` |
| `refinery` | 0.10.0 | 2026-10-02 | 1.7k stars, pushed 2026-11-01 |

All four projects are actively maintained right now; none is abandoned. Version churn differs
sharply though: sqlx is pre-1.0 (breaking minor releases), SeaORM just shipped its most
breaking major ever (2.0), Diesel is on its second stable major with a strict SemVer story.

---

## 1. SQLx (0.9.0)

**Async-nativeness — the strongest of the field.** SQLx is an async-first toolkit built on
futures; tokio is a first-class runtime (`runtime-tokio` feature; 0.9 also added smol /
async-global-executor support as async-std's successor). Queries are `async fn` throughout,
`Pool` is `Clone + Send + Sync` and designed to be shared across tasks (default
`max_connections` is 10 — raise it for the analytics/logging write path).

**Runtime queries vs compile-time macros.** Two families: the runtime `sqlx::query()` /
`query_as()` builders (no build-time checking), and the `sqlx::query!` / `query_as!` /
`query_file!` proc-macro family that type-checks SQL against a real schema at compile time,
inferring Rust output types (including nullability inference — for SQLite by stepping
through the prepared statement's `EXPLAIN` bytecode). 0.9 added `SqlSafeStr`: query strings
must be `&'static str` or wrapped in `AssertSqlSafe(..)`, a deliberate speed-bump against
`format!()`-built SQL (injection safety via a type).

**Offline CI without a database service — verified, it holds up.** `cargo sqlx prepare`
saves per-query metadata to a committed `.sqlx/` directory; the macros then compile without
any DB connection. `cargo sqlx prepare --check -- --all-targets --all-features` exits nonzero
if `.sqlx` drifts from queries or schema — explicitly "intended for use in Continuous
Integration". Setting `SQLX_OFFLINE=true` forces offline even when `DATABASE_URL` is present
(a footgun otherwise: presence of `DATABASE_URL` takes precedence over `.sqlx`). Caveats:
regeneration needs the DB *locally* (for SQLite that is just a file — cheap), and the FAQ
recommends pre-commit hooks / feature-gated queries need `-- --all-targets` to be captured.
As of 0.9 the offline machinery is behind the `offline` feature so it can be dropped entirely
to build without `serde`.

**Migrations.** Built in: `sqlx::migrate!()` embeds the migrations directory **into the
binary at compile time** (same model as `rust-embed` for the SPA), and `Migrator::run()`
applies them on startup with an `_sqlx_migrations` tracking table. 0.9 added `sqlx.toml`
config (rename/relocate the migrations table, per-crate `DATABASE_URL`), `no_tx` SQLite
migration support, and `Migrator::with_migrations()`.

**SQLite support quality.** `sqlite` feature enables `libsqlite3-sys/bundled`, which
"builds SQLite 3 from included source code and **statically links it into the final
binary**" — exactly what upland's single-binary constraint wants, and it tracks a much newer
SQLite than LTS distros. 0.9 moved `libsqlite3-sys` to a version **range**, so a rusqlite- or
sqlx-based crate in the same tree can unify onto one libsqlite3-sys. `SqliteConnectOptions`
exposes `journal_mode` (WAL), `busy_timeout` (default 5 s), `synchronous`, `foreign_keys`,
`locking_mode`, `pragma()` passthrough — all set per connection, so WAL + pragmas are just
pool configuration. The driver is runtime-agnostic at the connection level (SQLite has no
async API; SQLx runs each connection against an internal worker and `SqlitePool` uses the
runtime only for timeouts/management tasks). Note the honest caveat from Diesel's comparison
page: *"All async SQLite crates perform considerably worse than their sync counterparts"* —
true of sqlx's SQLite too, just irrelevant relative to an axum handler model.

**Postgres parity / escape cheapness.** Same crate, same API, second driver
(`runtime-tokio` + `postgres` feature). You keep hand-written SQL; the bind-parameter syntax
(`$1`) is even identical between SQLite and Postgres. The migration path is: enable the
`postgres` feature, swap `SqlitePool` → `PgPool`, write Postgres migrations, and re-run
`cargo sqlx prepare` against Postgres to regenerate `.sqlx`. Frictions: `query!` metadata is
per-backend (you can't check one query against both dialects simultaneously in one build —
0.9's `sqlx.toml` multi-database support eases the workspace story), and any SQL you wrote
that leaned on SQLite type-affinity quirks will surface as Postgres type errors. The
`Any`/runtime-selected driver exists but is documented "SEE DOCUMENTATION BEFORE USE", so
the escape is *one-dialect-at-a-time*, not dual-dialect.

**Deps / footprint.** `sqlx-sqlite` + bundled SQLite adds a C toolchain requirement on every
build host (relevant to the 3-target release matrix, not to end users). Proc-macro compile
times are the recurring community complaint — each `query!` call adds tens of ms and large
code bases report multi-minute builds; offline mode reduces this (no DB round-trips at
compile time). No controlled binary-size measurement was done for this report; the SQLite
static link is the dominant, shared cost across all candidates.

**Maintenance.** Healthy but mid-transition: 0.9.0 (May 2026) is the first release of the
collective-owned transact-rs era, MSRV 1.94, active PR flow, 781 open issues (high, but CI
and release cadence are live). Pre-1.0: breaking minor releases are the norm (0.9 changed
`Migrate` trait, `RawSql`, `Cow` impls, etc.) — budget for an upgrade pass per minor bump.
`cargo install --locked sqlx-cli` no longer works (Cargo.lock removed from the repo), so pin
sqlx-cli in CI by other means.

**Known gotchas (documented).**
- Compile-time checks need DB or `.sqlx`; a stale `.sqlx` silently passes builds against the
  *old* schema unless CI runs `prepare --check` against a live DB (i.e., the check job
  itself needs SQLite — cheap, but keep it as one job with a DB, then build jobs offline).
- Heavy `query!` macro use slows incremental builds; many teams mix checked macros for hot
  queries and `query()`/`query_unchecked` elsewhere — that trades away the safety exactly
  where it's applied.
- Prepared statements everywhere: DDL can't go through `query*!`; migrations are the
  supported DDL path.

---

## 2. Diesel (2.4.0) + diesel-async (0.10.0)

**Async-nativeness — the hard-requirement weak spot.** Diesel core is **synchronous by
design**. The async story is the separate `diesel-async` crate. As of 2026 diesel-async has
*real* async drivers for **PostgreSQL and MySQL/MariaDB only**; its SQLite support (added
0.5.0, still current in 0.10.0) is the `SyncConnectionWrapper` — a wrapper whose own docs
say: *"Internally this wrapper type will use `spawn_blocking` on tokio to execute the request
on the inner connection"*. The `sqlite` cargo feature literally expands to
`["diesel/sqlite", "sync-connection-wrapper"]`, and the wrapper pulls
`tokio/rt-multi-thread`. So "async Diesel on SQLite" = every statement hopped onto a blocking
thread-pool task, per query. It is correct and works under axum (with deadpool/bb8/mobc
pools), but it is not an async driver — it is an offload. Diesel's own comparison page goes
further, arguing async SQLite crates are strictly worse than sync ones and recommending sync
for SQLite-only apps (their example: deadpool-diesel + sync connections, i.e. you still pay
blocking-pool-on-async-worker care in handler code).

**Query builder ergonomics & type safety.** The best *type-system* safety in the field:
queries are built with a Rust DSL checked against a generated `schema.rs`; no database is
touched at compile time, so **`cargo build` works with zero DB, zero offline artifacts, zero
macro cache** — genuinely the nicest CI story of the three for compilation. Dynamic queries
stay checked (sqlx loses checking the moment a query is runtime-composed). Safety asymmetry
vs sqlx: diesel's checking doesn't know the live DB, so `schema.rs` staleness (you ran
migrations but didn't `diesel print-schema`) is a runtime error; and nullability inference
is arguably *better* than sqlx's (LEFT JOIN columns typed `Option` via the DSL, whereas sqlx
only heuristically infers it). Error messages are notorious — heavy trait proofs; diesel
ships a "how to read errors" doc and `#[diesel::debug_query]`-style hints, but this is a real
ergonomic tax. 2.4.0 added `#[derive(diesel::Enum)]`, Batch-Update for SQLite, a typed
`diesel::sqlite::pragma` module, `wal_checkpoint()`, commit/update/trace hooks, and a Rust
migration source in diesel-migrations.

**Migrations.** `diesel CLI` (`diesel_cli`) generates/applies SQL migrations; for
single-binary distribution `diesel_migrations::embed_migrations!` compiles them into the
binary — its own docs cite "want to ship a single executable file (such as for embedded
applications)". Running them against async connections needs the `migrations` feature
(0.7+ `AsyncMigrationHarness`) or `AsyncConnectionWrapper`. Gotcha: proc-macro can't watch
the migrations dir, so add a `build.rs` with `cargo:rerun-if-changed` or embedded migrations
go stale silently.

**Postgres parity / escape cheapness.** Ironically diesel's *escape* is excellent: the DSL is
backend-generic, so most DML is dialect-agnostic by construction, and diesel-async's
Postgres driver is a *real* async implementation (`tokio-postgres`-based, with query
pipelining). Switching SQLite→Postgres = change `SqliteConnection`→`AsyncPgConnection` types,
regenerate `schema.rs`, write Postgres-flavored migration SQL. Frictions: migration SQL is
per-backend (SQLite ALTER limitations mean your SQLite migrations and PG migrations can
diverge — you maintain both histories), and some APIs are backend-gated (`RETURNING`, arrays,
`INSERT ... ON CONFLICT` DSL variants), so code touching those gets `#[cfg]`-d during the
transition window.

**Deps / footprint.** Fewest moving parts at runtime (no sqlx-like macro layer; the sync
sqlite path uses the same bundled libsqlite3-sys C build as everyone). `diesel` +
`diesel-async` is two crates that must stay version-pinned (diesel-async 0.10 pins
`diesel ~2.4.0` exactly) — tight but coordinated.

**Maintenance.** The most disciplined of the field: Diesel 2.4.0 released 2026-11-13 (today),
2.3.x still receiving patches, 1.0 since 2018 with a real SemVer record, 166 open issues.
diesel-async ships same-day support for diesel minors.

**Verdict against the async gate.** Not *disqualified* — diesel-async+SyncConnectionWrapper
runs under axum and passes `Send` futures cleanly — but for an SQLite-first app it accepts
a per-query blocking-thread indirection that exists only to satisfy the async API shape.
If "async-native" is read as "the library is async-native, not just async-shaped", Diesel is
the only candidate that fails the letter of the requirement for the primary database.

---

## 3. SeaORM (2.0.4)

**Async-nativeness.** Async-first ORM built on top of… **SQLx** (sea-orm 2.0.4 depends on
`sqlx ^0.9.0` — already on the newest). For SQLite specifically, the async runtime layer is
literally sqlx's driver, so the async story is as native as sqlx's (including the same
"no async API in SQLite, driver-offloaded" nuance). Note SeaORM 2.0 also ships a *separate* `sea-orm-sync`
crate — the async codebase mechanically transpiled to synchronous, rusqlite-backed, for CLI
apps — a useful signal that SeaORM itself treats sync as the better SQLite fit for some
workloads, but irrelevant to upland's async server.

**Ergonomics: ActiveRecord vs free SQL.** The most "batteries-included" of the three: entity
codegen (`sea-orm-cli generate entity` — **requires a live database** to read the schema, a
sqlite file counts), ActiveModel CRUD, relations/eager-loading (`Entity::load().with(...)`),
model loader, nested ActiveModel, SeaQuery builder, plus `execute_raw`/`Command::from_sql`
escapes for free SQL. Safety is the weakest of the field: Diesel's own comparison puts
sea-orm in the "unchecked query builder" category — queries are syntactically plausible but
**not validated against the schema**; column/type mismatches are runtime errors. Nothing is
compile-time checked. For a config/credentials/analytics store where the queries are simple
and stable, that's livable; for the safety tie-break, SeaORM starts third.

**Migrations.** `sea-orm-migration`: migrations are **Rust code** (SeaQuery `Schema`
statements) in a dedicated crate, tracked in `seaql_migrations`, run at startup via
`Migrator::up(&conn, None)` — compiles into the binary, single-ship-friendly, and the DSL
generates dialect-appropriate SQL. Documented workflow is schema-first: write migration →
apply to live DB → regenerate entities. Known SQLite gaps surface here because SQLite's
limited `ALTER TABLE` shows through the abstraction — e.g. a documented failure of
add-column + drop-column on the same column in one pass on the SQLite backend
(sea-orm discussion #1929). 2.0's new entity format ("dense"), entity-first workflow and
`--er-diagram` codegen show real DX investment.

**Dialect lock-in vs Postgres escape.** Medium — better than most reviews say, with two
real costs. Upside: most of your code is SeaQuery/entity calls that compile to different SQL
per backend at runtime, so flipping `sqlx-postgres` + a `Database::connect(url)` change
moves *queries* over without rewriting them; entities regenerate against the new schema.
Costs: (1) every migration is written once in Rust but must work on both dialects (or be
guarded with backend checks) — the SQLite-ALTER sharp edges are yours either way; (2) raw-SQL
escapes and DB-specific types (JSON handling, booleans-as-integers affinity, `i64` vs SQLite
`INTEGER` width — note 2.0's `--big-integer-type` codegen flag is "intended for SQLite")
keep dialect knowledge in the code; (3) **dependency-chain lock-in**: SeaORM regularly bumps
its *public* dependency on sqlx in minor releases — Diesel's comparison calls this out as a
SemVer violation, and SeaORM 2.0→sqlx 0.9 churn is recent history — so the escape is cheap
in SQL terms but you sit on a three-level upgrade coupling (sea-orm→sqlx→tokio-compat).

**Deps / footprint.** Heaviest transitive tree of the field: sea-orm + sea-query +
sea-orm-macro + sqlx (macros) + ouroboros/strum/etc.; SeaQL's own numbers: their async
quickstart example took ~30 s to compile vs ~15 s for the sync/rusqlite variant. Same C
toolchain note for bundled SQLite. The `DatabaseConnection`/pool is `Clone + Send + Sync`
and shares fine across axum tasks (it wraps sqlx's pool).

**Maintenance.** Active: 2.0.0 in July 2026, 2.0.4 in September 2026, repo pushed this week,
commercial backing via SeaORM Pro, huge adoption (250k+ weekly). But: 2.0 is a breaking
major that just landed with migration guides still settling, and the 0.x→1.0→2.0 churn rate
is the highest of the three.

---

## Migration-tool alternatives (aside from the ORM's built-ins)

- **refinery 0.10.0** (Oct 2026, pushed Nov 2026): plain-SQL migrations with checksums, an
  embed macro, and backends for rusqlite (sync), **tokio-postgres** (async), mysql_async,
  tiberius. A good "no-ORM" path: rusqlite + `spawn_blocking` + refinery now, tokio-postgres
  + refinery later — the migration story survives the DB swap intact. Worth noting even if
  only as a fallback if the ORM choice is revisited.
- **sql-migrate**: the ticket named this, but `sql-migrate` is a **Go** tool
  (github.com/rubenv/sql-migrate, homebrew: "SQL schema migration tool for Go"); no such
  crate exists on crates.io (`sql-migrate` and `sql_migrate` both 404). The Rust-relevant
  analogue is `sqlx::migrate!` / `refinery` / `diesel_migrations` / `sea-orm-migration`.
  `golang-migrate` (Go binary, language-agnostic CLI) is the third-party CLI option if
  versioned SQL files with no library coupling are ever wanted.
- Every one of the three candidates has **compile-time-embedded, startup-run migrations**,
  which is the correct shape for upland's single binary; a separate tool buys little unless
  it decouples the migration runner from the query layer (refinery's argument).

## Single-binary / axum / tokio gotchas that apply to all three

1. **Bundled SQLite is a C build.** sqlx's `sqlite` feature (and diesel/rusqlite `bundled`)
   compile `sqlite3.c` and link it statically — good for shipping (self-contained binary),
   but every CI release job (linux-amd64, macos-arm64, windows-amd64) needs a working C
   compiler for its target (MSVC toolchain or `-gnu` for Windows; clang for macOS cross on
   Apple Silicon; cross-compilers/zig for single-host matrices). None of the three changes
   this; it's a property of `libsqlite3-sys`.
2. **One libsqlite3-sys in the tree.** sqlx 0.9 switched to a version *range* specifically so
   rusqlite/diesel/other sqlite users unify; if a dep forces an outside version you get
   resolution failures (documented on the sqlx sqlite page).
3. **SQLite + async == offloaded threads.** No async driver is truly async because SQLite
   exposes no async API; sqlx uses internal workers, diesel-async uses `spawn_blocking`,
   SeaORM inherits sqlx's model. Benchmark numbers from the Diesel repo confirm async SQLite
   is slower than sync — plan the WAL-mode + `busy_timeout` + single-writer discipline, not
   the crate choice, for log/analytics write throughput.
4. **Compile-time-checked schemas go stale differently.** sqlx: regenerate `.sqlx` after
   migrations (or CI `prepare --check` catches it *with* a DB). diesel: regenerate
   `schema.rs` via `diesel print-schema` (nothing catches it — runtime errors). sea-orm:
   regenerate entities against the migrated DB (same, but you also regenerate code).
5. **Pool sharing is identical in axum** (`State` + `Clone` pools) — no differentiator.

## Comparison against the criteria

| Criterion | SQLx 0.9 | Diesel 2.4 + async 0.10 | SeaORM 2.0 |
|---|---|---|---|
| **Async-native (tokio)** — hard gate | ✅ built async-first; sqlite driver offloads internally, ergonomic everywhere | ⚠️ core is sync; SQLite "async" = `spawn_blocking` wrapper, not an async driver (real async only on PG/MySQL) | ✅ async-first (delegates SQLite I/O to sqlx 0.9) |
| **Embedded-first single binary** | ✅ bundled static libsqlite3 (default); `migrate!` embeds migrations | ✅ same C-vendored story; `embed_migrations!` ships SQL in binary; needs `build.rs` for refresh | ✅ sqlx-bundled SQLite; Rust-code migrations in binary; entity gen needs a DB file |
| **Cheap Postgres escape** | ✅ same API/driver crate; rewrite `.sqlx` + migrations; SQL is yours to port | ✅ DSL is backend-generic; real async PG driver when you get there; per-backend migration SQL | ⚠️ query DSL ports, but dialect quirks + SemVer-broken sqlx dependency bumps couple upgrades |
| **Ergonomics & safety (tie-break)** | ✅ SQL stays visible; `query!` = compile-time verified, incl. nullability; proc-macro compile cost | ✅ strongest type-level checking, zero-DB builds; DSL verbosity + famous error messages | ⚠️ most productive day-1 (ActiveRecord, relations, codegen); **no compile-time query checking**; codegen workflow needs live DB |
| **Footprint / build time** | ⚠️ macros add build time (per-`query!` ms, well-documented); moderate tree | ✅ leanest tree; no build-time DB or offline data; fastest CI compile story | ⚠️ heaviest tree (~30 s vs ~15 s toy-app compile in SeaQL's own numbers) |
| **Maintenance** | ✅ active, new collective org; pre-1.0 breaking minors | ✅✅ best release discipline (2.4.0 today, 2.3.x LTS patches) | ✅ active, well-sponsored; just shipped a breaking major; churn highest |

## Where the field lands

- **Diesel does not cleanly pass the async hard gate for this app.** Its async crate is
  excellent *for PostgreSQL/MySQL*, but upland's primary store is SQLite, where diesel-async
  is a `spawn_blocking` facade around the sync connection, and Diesel's own docs recommend
  staying sync for SQLite. It survives as a candidate only if the developer's "async-native"
  is read as "integrates under tokio" rather than "driver is async-native". Safety and
  zero-DB CI are its genuine strengths; type-level DSL + per-backend migrations are its
  genuine costs here.
- **SeaORM's lock-in is a moderate dent in the Postgres escape, but its bigger issue is the
  safety tie-break:** as the ORM built on top of SQLx, it inherits all of SQLx's SQLite/PG
  mechanics, gives back ActiveRecord convenience, and removes compile-time query checking —
  while adding a codegen step that needs a live DB and a track record of SemVer-breaking
  sqlx bumps underneath. For a small team shipping a single binary, that's the middle on
  every axis: never worst, never best.
- **SQLx's offline CI workflow holds up.** `cargo sqlx prepare` + committed `.sqlx` +
  `prepare --check` in CI is documented, battle-tested, works for SQLite (the "DB" is a
  file), and 0.9 even made offline optional. The residual tax — regenerate metadata after
  schema changes, slower macro builds, and a pre-1.0 upgrade cadence — is the price of the
  best compile-time-checked free-SQL ergonomics available.
- **Shortlist:** **SQLx** as the front-runner (passes all three hard constraints outright
  and leads on the safety half of the tie-break with SQL staying first-class), **SeaORM**
  as the viable ergonomic alternative if ActiveRecord-style development is what the
  developer wants day-to-day (it is async-native and single-binary-clean; it just doesn't
  beat SQLx on any of upland's own stated priorities). **Diesel** lands third unless the
  async gate gets reinterpreted. A non-ORM wildcard worth one line in the decision ticket:
  **rusqlite + `spawn_blocking` + refinery** — Diesel's own performance argument says this
  is the fastest SQLite stack under async apps, at the cost of writing the repository layer.

*(No decision here — this is the field map for the decision ticket.)*

## Sources

Versions/dates via crates.io API (fetched 2026-11-13) and GitHub API (star/push counts,
2026-11-13):

- SQLx changelog (0.9.0, transact-rs move, `sqlx.toml`, `SqlSafeStr` #3723, libsqlite3-sys
  range #3928, offline optional #4077, MSRV 1.94):
  <https://github.com/transact-rs/sqlx/blob/main/CHANGELOG.md>
- SQLx 0.9 crate docs (runtime/TLS sections; "SQLite driver is runtime-agnostic"):
  <https://docs.rs/sqlx/latest/sqlx/index.html>
- `query!()` macro docs — requirements, nullability inference, Offline Mode,
  `prepare --check`, `SQLX_OFFLINE`: <https://docs.rs/sqlx/latest/sqlx/macro.query.html>
- SQLx FAQ — offline CI, macro internals (EXPLAIN bytecode nullability), docs.rs recipe:
  <https://raw.githubusercontent.com/transact-rs/sqlx/main/FAQ.md>
- sqlx-cli README — prepare / `--check` / feature-gated queries:
  <https://cdn.jsdelivr.net/gh/transact-rs/sqlx@main/sqlx-cli/README.md>
- sqlx SQLite driver module — bundled static linking (default), dynamic
  `sqlite-unbundled`, libsqlite3-sys range: <https://docs.rs/sqlx/latest/sqlx/sqlite/index.html>
- `SqliteConnectOptions` — `journal_mode`/WAL, `busy_timeout` (5 s default), `synchronous`,
  pragmas: <https://docs.rs/sqlx/latest/sqlx/sqlite/struct.SqliteConnectOptions.html>
- sqlx pool defaults (`max_connections: 10`):
  <https://cdn.jsdelivr.net/gh/transact-rs/sqlx@main/sqlx-core/src/pool/options.rs>
- sqlx per-`query!` compile-time cost report: <https://github.com/launchbadge/sqlx/issues/1096>
- Diesel changelog (2.4.0 dated 2026-11-13; SQLite pragmas/WAL/hooks/Batch-Update):
  <https://raw.githubusercontent.com/diesel-rs/diesel/master/CHANGELOG.md>
- diesel-async README (supported DBs; pools) —
  <https://raw.githubusercontent.com/weiznich/diesel_async/main/README.md>
- diesel-async changelog (0.5.0 "SyncConnectionWrapper … enables SQLite"; 0.7.0
  AsyncMigrationHarness; 0.10.0): <https://raw.githubusercontent.com/weiznich/diesel_async/main/CHANGELOG.md>
- diesel-async Cargo.toml (`sqlite = ["diesel/sqlite", "sync-connection-wrapper"]`,
  `diesel ~2.4.0`): <https://raw.githubusercontent.com/weiznich/diesel_async/main/Cargo.toml>
- `SyncConnectionWrapper` docs ("By default a tokio runtime and its `spawn_blocking` function
  is used"; SQLite-only support) and
  source: <https://docs.rs/diesel-async/latest/diesel_async/sync_connection_wrapper/index.html>,
  <https://cdn.jsdelivr.net/gh/weiznich/diesel_async@master/src/sync_connection_wrapper/mod.rs>
- `diesel_migrations::embed_migrations!` (single-executable rationale; build.rs
  caveat): <https://docs.rs/diesel_migrations/latest/diesel_migrations/macro.embed_migrations.html>
- Diesel "Compare Diesel" page (async-SQLite performance admission; sea-orm SemVer
  criticism; safety tiers; deadpool-diesel sync recommendation):
  <https://diesel.rs/compare/compare_diesel/>
- SeaORM 2.0 "What's New" index: <https://www.sea-ql.org/SeaORM/docs/introduction/whats-new/>
- SeaORM blog — "How we made SeaORM synchronous" (sea-orm-sync/rusqlite; 30s vs 15s compile
  numbers): <https://www.sea-ql.org/blog/2025-12-12-sea-orm-2.0/>
- SeaORM 2.0 Migration Guide (2026-01-12): <https://www.sea-ql.org/blog/2026-01-12-sea-orm-2.0/>
- sea-orm-cli generate entity (requires DATABASE_URL; `--big-integer-type … intended for
  SQLite`): <https://www.sea-ql.org/SeaORM/docs/generate-entity/sea-orm-cli/>
- SeaORM Setting Up Migration (Rust-code migrations, `Migrator::up` at startup):
  <https://www.sea-ql.org/SeaORM/docs/migration/setting-up-migration/>
- SeaORM SQLite migration bug (add+drop column same pass, SQLite):
  <https://github.com/SeaQL/sea-orm/discussions/1929>
- sea-orm 2.0.4 dependencies (`sqlx ^0.9.0`, `sea-query ~1.0.0`): crates.io API
- refinery 0.10.0 feature flags (rusqlite-bundled, tokio-postgres, mysql_async,
  tiberius): <https://docs.rs/crate/refinery/latest/features>
- sql-migrate is a Go tool, no Rust crate of that name (crates.io 404 for
  `sql-migrate`/`sql_migrate`): <https://formulae.brew.sh/formula/sql-migrate>,
  <https://github.com/rubenv/sql-migrate>
