# Research: can SQLite carry all four gateway workloads as an embedded store?

- **Issue:** xminihub/upland #5 (part of the persistence-stack map #4; feeds decision #7 "single engine or split storage?" and the ADR #10)
- **Date:** 2026-11-13
- **Scope:** SQLite (WAL) embedded in the single `upland` release binary under an axum/tokio async runtime, carrying: (1) config & credentials, (2) request/response logs, (3) usage analytics, (4) session/state. Constraints: async-native data layer is hard-required; a later migration to Postgres must stay cheap; developer ergonomics & safety are the top criteria.

## TL;DR verdict

| Workload | Can SQLite carry it embedded-first? | One-line condition |
| --- | --- | --- |
| 1. Config & credentials | **Yes — comfortably** | Read-heavy small relational data is SQLite's home turf; nothing special required. |
| 2. Request/response logs | **Yes, with strict conditions** — the workload that shapes the whole design | Separate `logs.db` file, single writer connection + app-level write queue with batched multi-row transactions, row-size cap (~≤1 MB, zstd-compress payloads), time-bucketed tables + `DROP` for pruning instead of mass `DELETE`. |
| 3. Usage analytics | **Yes at single-node self-host scale** | Don't scan raw logs interactively; maintain rollup tables (`usage_hourly`-style) with covering indexes; archive cold data to files a DuckDB sidecar can still read. |
| 4. Session/state | **Yes** | Point lookups are sub-ms; route the write leg through the same single-writer queue; avoid hot-row UPDATE storms with in-memory coalescing. |

**Overall:** at upland's shape — one process, one node, self-hosted gateway — SQLite can carry all four workloads embedded-first. The real split axis is **file-level, not engine-level**: a small `config.db` (with sessions in it or beside it) separate from a churned `logs.db`. The exit triggers that would argue for Postgres/ClickHouse are *multi-node HA*, *sustained ≫100 req/s with full prompt/completion retention*, or *interactive analytics over many months of raw logs* — none of which are upland's stated starting point. No near-neighbour engine (libSQL, Turso, DuckDB) changes the embedded verdict today: libSQL inherits the single-writer model; Turso's concurrent-writes rewrite is new; DuckDB is a complementary OLAP sidecar, not a hot-path store.

---

## 1. WAL-mode concurrency under async Rust

### 1.1 The locking model is the constraint, not the runtime

SQLite allows many concurrent readers and **exactly one writer** at a time; in WAL mode readers never block the writer and vice versa ([sqlite.org/wal.html](https://www.sqlite.org/wal.html), [sqlite.org/lockingv3.html](https://www.sqlite.org/lockingv3.html)). A write transaction holds the write lock for its *entire duration* — including any time the task is `await`-ing unrelated work — so long write transactions are "the killers of SQLite concurrency" [1][2].

Three subtleties that bite async apps specifically:

1. **`busy_timeout` waits for the *lock*, but a deferred transaction that upgrades read→write fails `SQLITE_BUSY` *immediately*, regardless of the timeout**, because the upgrade is guaranteed to conflict ([sqlite.org/lang_transaction.html](https://www.sqlite.org/lang_transaction.html), [2][3]). `PRAGMA busy_timeout` is per-connection — it must be set on every connection a pool opens.
2. **Tokio amplifies the trap.** A sqlx write transaction that yields at an `await` point parks a task *holding the exclusive lock*; other scheduled writers spin until their `busy_timeout` expires. Evan Schwartz measured this on a 50-connection `SqlitePool`: **2,586 rows/s with p99 of 182 s**, versus **60,061 rows/s and 82 ms p99 with a single-writer pool (`max_connections(1)`) plus a separate read-only pool** — ~20x — or ~equivalent performance from wrapping writes in a `tokio::sync::Mutex` [4]. (His update retracts "batching fixes it under contention" — batching helps per-statement overhead, but contention for the write lock is what must be removed.) sqlx's default `busy_timeout` is 5 s; the "slow statement: execution time exceeded alert threshold" warnings sitting at ≈ your busy_timeout are the fingerprint of this problem [4].
3. **Checkpoint starvation.** The WAL can only be checkpointed up to the end-mark of the oldest active reader; one long-running analytics `SELECT` keeps the WAL from truncating and it grows indefinitely ([sqlite.org/wal.html §6, "Avoiding Excessively Large WAL Files"](https://www.sqlite.org/wal.html)) [1][2]. Long reads and heavy writes interact through the WAL file even though they don't lock each other.

### 1.2 What the async-Rust ecosystem actually offers

- **sqlx (SQLite backend)** — async API over the synchronous C library via `libsqlite3-sys`; it is "async in API shape", calls are still serialized per connection [5][6]. Compile-time-checked queries (`query!`) work against SQLite but are **locked to one backend at compile time**, which matters for the eventual Postgres migration (see §6). Known ergonomic hazards around write transactions + pooling are documented in [4] (and a docs-warning PR [7]).
- **tokio-rusqlite** — rusqlite (sync, batteries-included `bundled` SQLite build) with an async handle: one dedicated background thread per `Connection`; closures are queued over an mpsc channel and awaited asynchronously [8]. This *structurally enforces* the single-writer shape (writes queue in one place), at the cost of moving closures/rows across the thread boundary and writing sync SQL callbacks.
- **Other shapes:** deadpool/`async-sqlite`-style pools run N background connections but still serialize on SQLite's write lock — "one writer connection (plus a handful of readers) is the correct shape, not a 20-connection pool; more writers just queue on the write lock" [9]. The Diesel docs' comparison page warns that all async SQLite crates measure slower than sync ones because SQLite has no async API — treat async SQLite as an ergonomics layer, not a throughput source [6].

### 1.3 Throughput ceiling for concurrent writers

Measured on a MacBook Pro M1, WAL + `synchronous=NORMAL`, one insert per transaction [1]:

| # threads | 1 | 8 | 32 | 64 | 128 | 256 |
| --- | --- | --- | --- | --- | --- | --- |
| ops/s (1 KB rows) | ~72k | ~72k | ~68k | ~68k | ~62k | **errors** |

So: **hundreds of tokio tasks is fine; dozens of concurrent *write transactions* is fine; ~100+ genuinely parallel writers degrade and fail without app-level serialization** — at which point an `std/tokio::Mutex` around writes makes "database is locked" impossible and throughput flat [1][2][4]. Bert Hubert covers the remaining `SQLITE_BUSY`-despite-timeout corner cases (BEGIN IMMEDIATE or single-statement transactions are the fixes) [3].

## 2. Request/response logs: MB-scale payloads and high write rates

### 2.1 Insert throughput at realistic payload sizes

Same benchmark setup (M1, WAL, `synchronous=NORMAL`, autocommit per insert) [1]:

| Row size | 10 B–1 KB | 10 KB | 100 KB | 1 MB | 10 MB |
| --- | --- | --- | --- | --- | --- |
| inserts/s | 70k–100k | ~24k | ~4k | **~450** | **~43** |

i.e. MB-scale inserts are **disk-bandwidth-bound (~400–450 MB/s effective)** and each row additionally pays WAL write-then-checkpoint double-write into the main file. Wrapping many inserts per transaction raises small-row throughput 2–20x [10]; for ≥1 MB rows batching mostly saves lock/txn overhead, not bytes. Context from the closest comparable, LiteLLM: metadata-only logging ≈ **1–2 KB per request**, storing prompts/completions in rows multiplies that **~10x**, and they explicitly advise keeping error logs *out* of the DB and batching spend writes at production scale [11].

For upland: typical agent-API calls are KB–low-hundreds-KB JSON. A gateway writing, say, 100 000 calls/day ≈ 100–300 MB/day of payload text — SQLite absorbs this with batching. **Sustained hundreds of MB/day is comfortable; sustained 10 MB/s streams are not this store's job.**

### 2.2 BLOB/TEXT storage practice and limits

- `SQLITE_MAX_LENGTH` default = **1 000 000 000 bytes**; hard implementation ceiling 2³¹−3; and **the whole *row* is encoded as one BLOB during INSERT/SELECT processing**, so the limit is per-row, not per-column ([sqlite.org/limits.html §1](https://www.sqlite.org/limits.html)) [12]. SQLite's own security advice is to *lower* it (to a few million) for server-ish workloads — a good fit for a gateway that should reject absurd payloads anyway (`sqlite3_limit(SQLITE_LIMIT_LENGTH)` is available from rusqlite/sqlx handles).
- Large values spill to **overflow pages** (linked list); reading `substr()` at offset *k* is O(k) — MB–GB blobs are not efficiently seekable ([sqlite.org/forum #3499345bf601e0b1](https://sqlite.org/forum/info/3499345bf601e0b1)) [13].
- SQLite stores blobs *faster than the filesystem does files* for small/medium objects ("35% Faster Than The Filesystem", avg 10 KB blobs random-read 2–10x faster than `open()/read()`; [sqlite.org/fasterthanfs.html](https://www.sqlite.org/fasterthanfs.html); the FAQ's "internal vs external BLOBs" gives the same guidance, [sqlite.org/faq.html#7.4](https://www.sqlite.org/faq.html#7_4)) [14]. The crossover where external files win is multi-MB streaming payloads and per-object lifecycle management.
- Schema hygiene for rows with a big TEXT/BLOB column: put index/lookup columns first, the payload column **last** (fields after a large blob need overflow-page traversal to decode) [15]. Separate the payload into its own table keyed by log rowid — exactly what agentgateway ships (`request_logs` metadata + `request_log_payloads` with prompt/completion JSON) [16] — and compress (zstd on JSON typically 5–10x) if retention is long.
- Theoretical caps (max DB size 281 TB at 64 KiB pages [12]) are not the practical constraint; **file bloat from freelist churn is** (below).

### 2.3 Retention / pruning patterns

How deletes behave: rows deleted go to the **freelist**; the file does not shrink, but pages *are reused by subsequent inserts* unless `auto_vacuum` is enabled ([sqlite.org/pragma.html](https://www.sqlite.org/pragma.html#pragma_auto_vacuum)) [17]. A rolling-append log table therefore largely recycles its own space in steady state. The classic failure mode is a write-then-delete-dominated DB where the live set is small: NetBird's `store.db` reached **337 MB with 88 % freelist** after months of daily log-`DELETE` cleanup (auto_vacuum=NONE, never shrunk; manual `VACUUM` reclaimed 298 MB in 0.7 s but needs an exclusive lock, i.e. a stopped writer) [18].

Patterns, in rough order of fit for upland:

1. **Time-bucketed tables (preferred):** write into `logs_2026_11`-style tables (or per-day); prune = `DROP TABLE`, which frees pages contiguously and can't fragment the live data the way mass `DELETE` does; archive is `ATTACH` + `INSERT … SELECT` or just shipping the detached single file. This also sidesteps the "DELETE scans the index for millions of rows" cost and keeps per-table indexes small for analytics ("query this month only").
2. **Batched `DELETE` + `PRAGMA incremental_vacuum(N)`** with `auto_vacuum=INCREMENTAL` set at DB creation — runs in ms, doesn't take an exclusive lock, truncates freelist pages from EOF; tradeoff is fragmentation and WAL churn proportional to pages moved [17][18][19][20].
3. **Occasional `VACUUM`** after large deletes when file size matters — rewrites the whole file under an exclusive lock; or `VACUUM INTO 'file'` for compact online backups [17][19].
4. Separate **`logs.db` from `config.db`** so log churn, vacuum, checkpoint tuning, and future export never touch the credentials/config file (agentgateway likewise allows a dedicated request-log database [16]; new-api exposes `LOG_SQL_DSN` precisely to split logs off the main DB [21]).
5. **PRAGMA hygiene:** `journal_mode=WAL` (persistent, set once), `synchronous=NORMAL`, per-connection `busy_timeout≥5000`, `journal_size_limit` (e.g. 64 MB) + periodic `wal_checkpoint(TRUNCATE)` so a big transaction can't leave the WAL file bloated [10][2]; `PRAGMA optimize` on shutdown and `ANALYZE` after bulk loads [19]; keep reader transactions short to avoid checkpoint starvation (§1.1).

## 3. Usage analytics: ergonomics and the row-store tax

- SQLite executes aggregates correctly but with **row-at-a-time, single-threaded iteration**. Independent comparisons consistently report **~100x gaps vs DuckDB** for GROUP BY/scans over millions of rows (11.2M-row benchmark; DuckDB parallel, vectorized, columnar; [fastero](https://fastero.com/blog/duckdb-vs-sqlite-for-analytics-2026), [posthog](https://posthog.com/blog/duckdb-vs-sqlite), [motherduck](https://motherduck.com/learn/duckdb-vs-sqlite-databases), [HN/DuckDB sqlite_scanner](https://news.ycombinator.com/item?id=31355336)) [22]. The gap grows with data volume and query complexity [23].
- At single-node gateway scale (thousands–millions of rows/day, dashboards over days/weeks), raw-log aggregates are fine **with covering indexes** on `(ts, key_id, model)`-style shapes — but an interactive dashboard scanning months of raw rows on the same file that's serving live traffic is a latency hazard *and* a checkpoint-starvation hazard (§1.1).
- The right pattern is **pre-aggregated rollup tables** updated incrementally (triggers or batched upsert at flush time) — the industry-standard OLTP trick, and the conceptual inverse of what Langfuse did at SaaS scale: they outgrew Postgres row-updates/tracing at billions of rows and moved logs+analytics to ClickHouse, with S3 for payloads [24][25]. That trajectory (Postgres → ClickHouse) tells you analytics *is* the workload that breaks row-stores last, but only at cloud scale — which upland isn't at embedded-first.
- **FTS5** covers "find calls whose response said X": external-content virtual tables index a payload table without duplicating text, kept in sync by three standard triggers; contentless (`content=''`) tables are smaller but can only return rowids ([sqlite.org/fts5.html §4.4](https://www.sqlite.org/fts5.html), [coddy](https://coddy.tech/docs/sqlite/full-text-search)) [26]. Indexing every log body costs real write amplification — opt in per column, not by default.
- **Escape hatch that doesn't change the verdict:** DuckDB's `sqlite_scanner`/`ATTACH` reads a SQLite file directly (read-only, out-of-band) for heavy ad-hoc analytics over archived logs — "SQLite is your OLTP engine, DuckDB is your OLAP engine, same process even" [22][27]. DuckDB itself is also single-writer and bad at per-row updates [23], so it is *not* a candidate hot-path store.

## 4. Session/state: the hot path

Per-request key→row lookups are exactly SQLite's strength: PK lookups run from page cache in **single-digit µs to tens of µs**, and WAL lets them proceed concurrently with the writer [1][10]. Writes are the only serialization point: route session writes through the same single writer queue (§1.2/§5); coalesce hot-row increments in memory and flush periodically — LiteLLM's production guidance for exactly this shape (per-key spend UPDATE/UPSERT contention caused deadlocks/connection exhaustion *even on Postgres* above ~1k RPS; they batch and buffer) [11][28]. TTL-style state: a `expires_at` column + cheap scheduled sweep on a time-bucketed/`INTEGER PRIMARY KEY` table is enough at this scale; note SQLite has no background job scheduler — the app owns it. agentgateway stores its generated MCP session key in the same SQLite config DB [29], showing session-adjacent state living happily in the embedded store.

## 5. What comparable systems actually store where

| System | Stack | What they persist in SQLite | Signal for upland |
| --- | --- | --- | --- |
| **agentgateway** ([agentgateway.dev](https://agentgateway.dev/docs/standalone/latest/documentation/setup/storage/)) | Rust AI/agent gateway, single binary | Config DB (providers, **API keys**, policies, MCP targets) *and* `request_logs` + payload table; Postgres optional for HA/multi-node [29][16] | Closest analogue ships **all four workload classes on SQLite** by default; splits logs DB as an option, Postgres only for multi-replica. |
| **new-api / one-api** ([GitHub](https://github.com/QuantumNous/new-api)) | Go LLM gateway | **SQLite is the default** main DB (unset `SQL_DSN`); MySQL/PG for scale; separate `LOG_SQL_DSN` (SQL or **ClickHouse**) exists for request logs [21] | Even the "runnable from one binary" camp keeps the *logs* splittable first. |
| **LiteLLM** ([db_sizing](https://docs.litellm.ai/docs/proxy/db_sizing), [prod](https://docs.litellm.ai/docs/proxy/prod)) | Python gateway (Rust migration underway) | `.litellm/litellm.db` SQLite fallback exists; docs steer Postgres+Redis for keys/budgets/spend logs; SQLite called fine "for a laptop demo" because budget decrements race [30][11][28] | Config+session reads are trivially SQLite-able; the racing pattern is **hot-row UPDATE**, fixed by batching/coalescing — which works identically in SQLite. |
| **Langfuse** ([v3 infra blog](https://langfuse.com/blog/2024-12-langfuse-v3-infrastructure-evolution), [ClickHouse case study](https://clickhouse.com/blog/langfuse-llm-analytics)) | LLM observability | Postgres → **ClickHouse + S3** once tracing hit billions of events; row-update-heavy trace ingestion was the break point [24][25] | Where log+analytics workloads go at *SaaS* scale; validates "don't fight row stores at that volume", irrelevant at upland's embedded scale. |
| **Atuin** ([docs](https://docs.atuin.sh/18.17/self-hosting/server-setup), [source via forum](https://forum.atuin.sh/t/changing-the-host-associated-with-commands-in-history/888)) | Rust single-binary shell-history client + sync server | Client: sqlx + SQLite WAL for history, *plus a separate `records.db`/`init-store`* for the sync-log store; server: Postgres-first, SQLite supported [31][32] | Proof of async-Rust SQLite under an interactive concurrent-write workload (every shell command), and of **splitting stores by role** — a separate file for the append-heavy component. |
| **Grafana 12** ([install docs](https://grafana.com/docs/grafana/latest/setup-grafana/installation)) | Go observability server | New installs **default to embedded SQLite** for users/dashboards/config; officially "not recommended for production", HA requires MySQL/PG [33][34] | Config-DB workload on SQLite is mainstream; the "production" caveat is about HA and write scale, not the workload shape. |
| **dstack** | Python infra gateway | SQLite datastore whose concurrent-write problems motivated the [1] benchmarks; app-level mutex over the writer [1] | Pattern 1.2's prescription in the wild. |

Also relevant to the operational side: the Fly.io "all-in on SQLite" server-side experiment (LiteFS/LiteStream) has pulled back — LiteFS Cloud was sunset in 2025 with Litestream and managed Postgres named as the successors [35][36]. The honest niche that survived is **single primary + continuous backup**: Litestream streams WAL changes to S3-compatible storage for any SQLite app with no code changes [37], and fits upland's one-binary-one-volume topology naturally.

## 6. Keeping the Postgres migration cheap

The migration stays cheap if the design obeys a handful of rules *now*:

- **Portable SQL, no SQLite-isms in the data layer.** Both engines support `INSERT … ON CONFLICT … DO UPDATE` (SQLite adopted Postgres' upsert syntax in 3.24) and `RETURNING` (SQLite 3.35+); keep `json_extract`/FTS5/pragmas behind repository boundaries so they can be re-implemented per backend [38][39].
- **Schema/type discipline:** `INTEGER` rowids → `BIGINT`/`BIGSERIAL` later; store datetimes as ISO-8601 `TEXT` or unix-ms `INTEGER` (pgloader maps either sanely); BLOB → `BYTEA`; remember SQLite applies type *affinity* rather than strict enforcement, so Postgres will surface lax typing as errors — the fix is strict `NOT NULL`/`CHECK` typing from day one [40].
- **Migration mechanics:** `pgloader` moves schema+data from a live SQLite file to Postgres and is the documented path for exactly this class of move (Neon's SQLite→Postgres guide; the Grafana community uses the same tool) [40][34]. Data volume makes the split-file design (§2.3/§5) migration-friendly: config.db migrates in seconds; logs.db is usually *retired*, not migrated.
- **Ergonomics tax to flag for issue #6 (driver choice):** sqlx's compile-time `query!` macros bind to one backend per build — dual-backend checking means maintaining two checked query sets or dropping to runtime-checked `query()`; Diesel similarly needs backend-generic code or conditional compilation; SeaORM abstracts both. If "cheap migration to Postgres" is weighted heavily, the safest shape is a hand-written repository layer over plain SQL strings + a migrations tool that runs on both engines (e.g. refinery), independent of which driver wins #6. (This is a note, not a verdict — #6 owns that decision.)

## 7. Conditions that make SQLite viable here (recommended posture)

1. **Connection shape:** one writer (tokio-rusqlite handle, or sqlx pool `max_connections(1)`) with writes queued at the app level; separate read-only pool for the many readers [4][8][1]. Never hold a write transaction across non-DB awaits; prefer single-statement transactions; `BEGIN IMMEDIATE` when a txn must read-then-write [2][3].
2. **Pragmas (per connection):** `busy_timeout=5000+` (sqlx default 5 000 ms is sane; some production systems use 10–20 s [1][4]); database-level `journal_mode=WAL`, `synchronous=NORMAL`, `auto_vacuum=INCREMENTAL` (decided at creation) or none+manual vacuum, `journal_size_limit≈64 MB`, bounded `SQLITE_LIMIT_LENGTH`, generous `mmap_size`, `PRAGMA optimize` on shutdown [1][2][4][10][12][17][19].
3. **Logs path:** drain an unbounded `tokio::mpsc` in the writer with 50–100 ms / N-MB batches into multi-row `INSERT`s; metadata columns first, payload last or in a payload table; zstd-compress payloads; reject/truncate beyond the cap; time-bucketed tables with `DROP TABLE` pruning + detached-file archiving; retention knob per tenant, default bounded [1][10][11][16][18].
4. **Analytics path:** rollup tables maintained at flush time (never `UPDATE` counters in the same txn as request rows), covering indexes for dashboards, FTS5 only where search is a product feature, DuckDB-over-archives as the scale escape hatch [22][26][27].
5. **Operations:** Litestream-style continuous WAL replication to object storage, or `VACUUM INTO` snapshots; single-process topology (a second writer host would break WAL's file-local locking — WAL is explicitly not for NFS/shared disks [2][41]).

## 8. Danger zones, honestly

- **Write contention** — not "can't handle it", but *unqueued* writers: 100+ parallel write transactions degrade/lose; parked-while-holding-the-lock async transactions are pathological (p99 ≈ busy_timeout, then errors) [1][4].
- **Checkpoint starvation** — one long analytics reader silently grows the WAL without bound; keep readers short or snapshot on `wal_checkpoint(TRUNCATE)` [2][1].
- **DB bloat & freelist churn** — the NetBird failure mode (90 % of file dead) for DELETE-dominated DBs; large `VACUUM` needs an exclusive pause [18][17].
- **Payload row-size cliffs** — MB-scale inserts cost the disk bandwidth twice (WAL + checkpoint) and mid-row seeks are O(k); beyond a few MB per object, files beat rows [1][13][14].
- **Analytics scans at scale** — the 100x columnar gap plus lock-free-but-IO-contending scans while serving traffic; rollups are mandatory, raw scans are the exit trigger toward DuckDB/ClickHouse [22][24].
- **Multi-node / HA** — WAL is one-process-one-file; anything wanting two live writers or read replicas is, by definition, the Postgres moment (and every comparable gateway draws the line there: new-api, LiteLLM, Grafana, agentgateway's Helm mode) [21][11][33][29].

## 9. Verdict

**SQLite as embedded store: yes for all four workloads at upland's constraints — provided the single-writer-queue + batched-log + rollup + time-bucketed-pruning posture above is treated as architecture, not tuning.** The workload that most shapes the design (and is first to argue for *storage splitting*) is **request/response logs**: it dictates the separate `logs.db` file, the batched writer path, and the pruning strategy. The workloads that would argue for *engine* changes (Postgres/ClickHouse) are the same ones that broke the comparables — sustained SaaS-scale log ingestion and deep interactive analytics (Langfuse), and multi-replica HA (LiteLLM/Grafana/new-api) — none of which upland's embedded-first premise includes. Migration cheapness is preserved by portable SQL + strict types + repository boundaries, which costs nothing now and pays off in whatever issue #7/#8/#10 decide.

## Sources

1. Victor Skvortsov, "SQLite concurrent writes and 'database is locked' errors" (benchmarks, recipe, M1 throughput table) — https://tenthousandmeters.com/blog/sqlite-concurrent-writes-and-database-is-locked-errors
2. SQLite docs, "Write-Ahead Logging" (readers/writers, checkpoint starvation §6, SQLITE_BUSY cases §9) — https://www.sqlite.org/wal.html
3. Bert Hubert, "A brief post on SQLITE_BUSY / database is locked despite timeout" — https://berthub.eu/articles/posts/a-brief-post-on-sqlite3-database-locked-despite-timeout
4. Evan Schwartz, "PSA: Your SQLite Connection Pool Might Be Ruining Your Write Performance" (sqlx numbers: 50-conn pool 2.6k rows/s vs single-writer 60k rows/s; mutex fix; busy_timeout default) — https://emschwartz.me/psa-your-sqlite-connection-pool-might-be-ruining-your-write-performance ; repro: https://github.com/emschwartz/sqlx-sqlite-contention-repro ; r/rust thread: https://www.reddit.com/r/rust/comments/1r7eh9v/psa_write_transactions_are_a_footgun_with_sqlx/
5. sqlx SQLite driver docs (bundled libsqlite3-sys; static linking) — https://docs.rs/sqlx/latest/sqlx/sqlite/
6. Diesel, "Compare" page (async SQLite crates slower than sync counterparts) — https://diesel.rs/compare/compare_diesel ; rusqlite issue #697 (blocking IO in async; sqlx does IO on background threads) — https://github.com/rusqlite/rusqlite/issues/697
7. sqlx docs-warning PR — https://github.com/launchbadge/sqlx/pull/4166
8. tokio-rusqlite crate docs (dedicated thread per connection, mpsc command queue) — https://docs.rs/tokio-rusqlite/latest/tokio_rusqlite/
9. "Rust ORMs in 2026: Diesel vs SQLx vs SeaORM vs Rusqlite" (correct shape = one writer + handful of readers; r2d2/deadpool notes) — https://www.rustfinity.com/blog/rust-orms ; Mako guide (same guidance) — https://mako.ai/guides/sqlite/connect-from-rust
10. PowerSync, "SQLite Optimizations For Ultra High-Performance" (baseline throughput; transactions 2–20x; WAL growth & journal_size_limit; vacuum/ANALYZE/optimize) — https://powersync.com/blog/sqlite-optimizations-for-ultra-high-performance
11. LiteLLM docs, "Database Sizing" (1–2 KB/request, ~10x with prompts, batch spend writes, keep error logs out of DB, retention) — https://docs.litellm.ai/docs/proxy/db_sizing ; "What is stored in the DB" — https://docs.litellm.ai/docs/proxy/db_info
12. SQLite docs, "Implementation Limits" (SQLITE_MAX_LENGTH 1e9 default / 2^31−3 ceiling / row-as-blob encoding; max DB size 281 TB; lowering limits server-side) — https://www.sqlite.org/limits.html
13. SQLite forum, "can I insert an 8GB file into sqlite?" (overflow-page linked list; O(k) mid-blob seek) — https://sqlite.org/forum/info/3499345bf601e0b1
14. SQLite docs, "35% Faster Than The Filesystem" (10 KB blobs, internal vs external) — https://www.sqlite.org/fasterthanfs.html ; FAQ §7.4 internal vs external BLOBs — https://www.sqlite.org/faq.html#7_4
15. SQLite forum, "Increasing insert performance of an in-memory db" (column order around blob/overflow decoding) — https://sqlite.org/forum/info/219fff0ae36f0ddd
16. agentgateway, "Store logs in a database" (SQLite/PG request_logs table; payloads in `request_log_payloads`; separate log DB option) — https://agentgateway.dev/docs/standalone/latest/documentation/observability/access-logs/database
17. SQLite docs, PRAGMA reference (auto_vacuum modes, freelist reuse, incremental_vacuum) — https://www.sqlite.org/pragma.html#pragma_auto_vacuum
18. NetBird discussion #6701, "store.db grows unbounded — DELETEs without VACUUM/incremental_vacuum" (337 MB @ 88 % freelist; 0.7 s manual VACUUM; production retention-cleanup case study) — https://github.com/netbirdio/netbird/discussions/6701
19. Coddy docs, "ANALYZE & VACUUM" (incremental_vacuum cadence; VACUUM INTO; PRAGMA optimize) — https://coddy.tech/docs/sqlite/analyze-and-vacuum
20. "Vacuuming the WALs" (incremental_vacuum WAL growth behavior, granular N) — https://www.theunterminatedstring.com/sqlite-vacuuming
21. new-api README (SQLite default when `SQL_DSN` unset; MySQL/PG options; `LOG_SQL_DSN` separate log DB incl. ClickHouse) — https://github.com/QuantumNous/new-api
22. DuckDB-vs-SQLite analytics comparisons: fastero benchmark writeup (11.2M rows, ~100x) — https://fastero.com/blog/duckdb-vs-sqlite-for-analytics-2026 ; Posthog deep dive — https://posthog.com/blog/duckdb-vs-sqlite ; MotherDuck FAQ — https://motherduck.com/learn/duckdb-vs-sqlite-databases ; HN on sqlite_scanner (DuckDB reads SQLite files directly) — https://news.ycombinator.com/item?id=31355336
23. Aleksei Aleinikov, "DuckDB vs SQLite, Polars & BigQuery" (workload shapes; DuckDB single-writer too) — https://www.alekseialeinikov.com/en/blog/topics/data/duckdb-explained-vs-sqlite-polars-bigquery
24. Langfuse, "From Zero to Scale: v3 infrastructure evolution" (Postgres→ClickHouse; S3 payloads; row-update cost in CH) — https://langfuse.com/blog/2024-12-langfuse-v3-infrastructure-evolution
25. ClickHouse blog, "How Langfuse is scaling LLM observability" (billions of events; minutes→sub-second) — https://clickhouse.com/blog/langfuse-llm-analytics
26. SQLite docs, "FTS5 Extension" §4.4 (external content & contentless tables, trigger pattern) — https://www.sqlite.org/fts5.html ; practical writeup — https://coddy.tech/docs/sqlite/full-text-search
27. dbpro, "DuckDB vs SQLite: Choose the Right Embedded Database" (keep OLTP in SQLite, scans in DuckDB; Parquet export) — https://www.dbpro.app/blog/duckdb-vs-sqlite
28. LiteLLM, "Production Best Practices" (spend-write hot rows cause deadlocks at ~1k+ RPS even on Postgres; Redis transaction buffer; batching) — https://docs.litellm.ai/docs/proxy/prod ; default-SQLite confusion in issue #30328 — https://github.com/BerriAI/litellm/issues/30328
29. agentgateway, "Configuration storage" (SQLite config DB stores providers, API keys, policies, MCP targets, session key; Postgres for multi-replica Helm mode) — https://agentgateway.dev/docs/standalone/latest/documentation/setup/storage/
30. StackHarbor, "Deploying LiteLLM proxy…" ("SQLite is fine for a laptop demo… budget decrements race under concurrent requests"; Postgres required for production) — https://stackharbor.com/en/knowledge-base/litellm-proxy-budgets-deploy
31. Atuin docs, "Server Setup" (Postgres 14+ first; SQLite supported via `db_uri=sqlite:///…`) — https://docs.atuin.sh/18.17/self-hosting/server-setup
32. Atuin client internals via community forum (sqlx `.journal_mode(SqliteJournalMode::Wal)` in `crates/atuin-client/src/database.rs` and `record/sqlite_store.rs`; separate `records.db` / `atuin history init-store`) — https://forum.atuin.sh/t/changing-the-host-associated-with-commands-in-history/888
33. Grafana docs, "Install Grafana" (new installs default to embedded SQLite; not recommended for production; HA requires MySQL/PG) — https://grafana.com/docs/grafana/latest/setup-grafana/installation
34. Grafana docs, "Set up for high availability" + community migration writeup (pgloader sqlite→postgres) — https://grafana.com/docs/grafana/latest/setup-grafana/set-up-for-high-availability ; https://medium.com/@yashraj45dighe/migrate-grafana-sqlite-db-to-postgresql-for-high-availability-c7303c3750ff
35. Fly.io community, "Sunsetting LiteFS Cloud" (LiteFS Cloud discontinued; Litestream/managed Postgres pointed to instead) — https://community.fly.io/t/sunsetting-litefs-cloud/20829
36. Fly blog, "All in on SQLite" (original server-side SQLite+LiteStream thesis, historical context) — https://fly.io/blog/all-in-on-sqlite-litestream/
37. Litestream (continuous WAL streaming replication/backup of a live SQLite file, no code changes) — https://litestream.io
38. SQLite docs, "UPSERT" (adopted Postgres ON CONFLICT syntax in 3.24, 2018) — https://www.sqlite.org/lang_UPSERT.html
39. SQLite docs, "RETURNING" clause (3.35+, Postgres-compatible shape) — https://www.sqlite.org/lang_returning.html
40. Neon docs, "Migrate from SQLite to Postgres" (pgloader schema+data mapping incl. types/sequences) — https://neon.com/docs/import/migrate-sqlite ; pgloader — https://pgloader.io
41. SQLite docs, "How To Corrupt An SQLite Database File" (WAL + NFS/shared-network-filesystems warning) — https://www.sqlite.org/howtocorrupt.html
