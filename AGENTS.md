# AGENTS.md

## Development

- `cargo fmt` — format (CI enforces `cargo fmt --check`)
- `cargo clippy --locked --all-targets` — lints; CI runs with `RUSTFLAGS: -Dwarnings`, so warnings fail the build
- `cargo test --locked` — full suite (unit + HTTP integration tests)
- `cargo run` — start the API server on `127.0.0.1:8080` (`--host`/`--port`, `UPLAND_HOST`/`UPLAND_PORT`)

Persistence (ADR-0003): embedded SQLite through SQLx at the **database path** (`--db-path`/`UPLAND_DB_PATH`, per-user data directory by default). Queries are compile-time-checked macros against committed offline metadata: after adding a query or a migration, run `cargo sqlx prepare` locally (needs `sqlx-cli --no-default-features --features sqlite`) and commit `.sqlx/`; builds and tests run offline via `.env` (`SQLX_OFFLINE=true`) and need no database service.

Frontend (`web/`, Bun; CI pins bun 1.4.4):

- `bun install` — install (lockfile `web/bun.lock` is committed; use `--frozen-lockfile` in CI)
- `bun run lint` — oxlint; `bun run format:check` — Prettier (fix with `bun run format`); both enforced in CI
- `bun run test` — Vitest (jsdom, app-root seam, co-located `src/**/*.test.tsx`); enforced in CI
- `bun run build` — typecheck + production build into `web/dist`
- `bun run dev` — Vite dev server on :5173, proxies `/api` to 127.0.0.1:8080

All HTTP behavior is tested through the in-process router seam (see `tests/`); tests must not open real sockets. SPA-serving tests skip when `web/dist` is empty; run `bun run build` first to exercise them locally. CI's Rust lane stages no bundle, so they skip there by decision; the `packaging` merge job asserts the crate carries the bundle the `web` lane built (`scripts/assert-crate-packaging.sh`).

Distribution (`scripts/`, shell + jq):

`release.yml` is one line — `verify → release → build → package → github-release → tap → cargo` — and every job's logic is one of these scripts; the YAML is wiring. CI's `packaging` job parse-checks every file here (`bash -n`) before running the contracts, but what a release script truly proves, it proves on a real release.

- `scripts/targets.json` — the target table: the release matrix and every asset name derive from this one file; `verify` validates it as its first step, so there is no reader job
- `scripts/check-targets.sh` — the target table's contract in one exit code (ADR-0001): `verify` runs it before any toolchain minutes are spent, and its stdout is the matrix JSON the build jobs consume
- `scripts/asset-name.sh` — the only place an asset name is spelled *from the table*; the installer re-spells it inline (no repo on a user's machine) — a deliberate exception, see ADR-0006
- `scripts/package-brew-tarball.sh` — one deterministic tarball per Homebrew target, driven once per brew row by `pack-assets.sh`; needs GNU tar (`brew install gnu-tar`, then `TAR=gtar`)
- `scripts/bump-and-tag.sh` — the release's bump in one call: set-version, commit with the bot's identity, tag, push; it runs only after every gate is green, because a red release must leave no version to explain (ADR-0001)
- `scripts/build-target.sh` — one table row as build plus smoke: zigbuild for the `*-musl` triples and cargo otherwise, deployment target off the row, and the artifact must run and name the released version before anything packages it (ADR-0001); the same call serves Windows runners under Git Bash
- `scripts/pack-assets.sh` — the release's asset set: tarballs per brew target, the named executable per exe target, the installer byte-for-byte, `SHA256SUMS`, and the structured notes body — all derived from the manifest records it writes as it goes, one derivation per fact (ADR-0001); the installer stays out of the records (transport, not artifact, ADR-0006), and the notes file lives outside `dist/assets` so it can never be uploaded
- `scripts/assert-assets.sh` — the asset guard: one asset per target in the table and every hash re-checked against the manifest; a red package job, not a release with a 404 waiting
- `scripts/install-musl-toolchain.sh` — pins and installs zig and `cargo-zigbuild`, the C toolchain the two `*-musl` targets build the bundled SQLite and link with; `release.yml` runs it before those builds
- `scripts/install.sh` — the **installer** (ADR-0006): POSIX sh, Linux/macOS, checksum-verified, installs to `~/.local/bin`; shipped byte-for-byte as the versionless `install.sh` release asset and deliberately ungated by CI — its first run against real assets is its first test
- `scripts/publish-crate.sh` — the **cargo channel**: commits the staged `web/dist` in the job's own checkout (cargo refuses to publish packaged files git hasn't committed, and `--allow-dirty` would waive that check on everything else), asks the sparse index so an already-published version skips instead of failing, then publishes. The release's `cargo` job runs it for real; CI's `packaging` job runs it with `--dry-run`, which is what proves it before a tag exists
- `bash scripts/test-package-brew-tarball.sh` — the packaging contract, asserted; CI's `packaging` merge job (Web ∥ Rust → Packaging → Gate) runs it on Ubuntu, and it is deliberately not a `cargo test`
- `scripts/ci-gate-summary.sh` — the `gate` job's whole content: renders the run summary and colours the gate from the lane results; `bash scripts/test-ci-gate-summary.sh` asserts that contract from the same shell-contract seam, run beside the tarball contract
- `scripts/bump-tap.sh` — the tap's contract in one exit code: dispatches the org tap's `bump.yml`, recognises the run by its job name, watches it, then reads back `Formula/upland.rb` — the pin, not the run's conclusion, is the verdict (ADR-0001); the job's `timeout-minutes` is what bounds the watch
- `Formula/upland.rb` and the Ruby template live in `xminihub/homebrew-tap`, not here; the release dispatches that repository's `bump.yml` and waits for it via `scripts/bump-tap.sh` (ADR-0001). The tap's upland seed files (`tools/upland.json`, `templates/upland.rb.tpl`) were onboarded per ADR-0006
- The release chain reads as the channel priority: `github-release → tap → cargo`. The cargo job stages the `web-dist` artifact into `web/dist`, asserts the crate listing, and publishes with the `CARGO_REGISTRY_TOKEN` secret (publish-only, must exist before the first release carrying this job); an already-published version is skipped, not failed — registry versions are one-shot (ADR-0006)

## Agent skills

### Issue tracker

Issues are tracked as GitHub Issues on `xminihub/upland` via the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Domain docs

Single-context: `CONTEXT.md` + `docs/adr/` at the repo root. See `docs/agents/domain.md`.

### Web UI skills

`shadcn` and `migrate-radix-to-base` apply only to the frontend in `web/`. Load them as soon as any subtask touches `web/` UI code — including mixed Rust+UI tasks once the backend part is done. Do not load them for purely backend, API, or packaging work.

All shadcn CLI commands must run with `web/` as the working directory using Bun (`cd web && bunx --bun shadcn@latest ...`); `components.json` lives in `web/`, not the repo root. If the skill's "Current Project Context" was generated from the repo root, re-run `info --json` from `web/` before trusting it.

### Language

Write all markdown you generate for this repo — specs, issues, task files, `CONTEXT.md`, ADRs, reports — in **English**. Code, commands, file paths, identifiers, and quoted error messages stay verbatim. This applies to new output only; don't translate existing docs.
