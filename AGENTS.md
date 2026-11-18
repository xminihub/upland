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

All HTTP behavior is tested through the in-process router seam (see `tests/`); tests must not open real sockets. SPA-serving tests skip when `web/dist` is empty; run `bun run build` first to exercise them locally (the CI test job consumes the frontend job's build artifact).

Distribution (`scripts/`, shell + jq):

- `scripts/targets.json` — the target table: the release matrix and every asset name derive from this one file
- `scripts/asset-name.sh` — the only place an asset name is spelled
- `scripts/package-brew-tarball.sh` — one deterministic tarball per Homebrew target; needs GNU tar (`brew install gnu-tar`, then `TAR=gtar`)
- `bash scripts/test-package-brew-tarball.sh` — the packaging contract, asserted; CI's `packaging` job runs it on Ubuntu, and it is deliberately not a `cargo test`
- `Formula/upland.rb` and the Ruby template live in `xminihub/homebrew-tap`, not here; the release dispatches that repository's `bump.yml` and waits for it (ADR-0001)

## Agent skills

### Issue tracker

Issues are tracked as GitHub Issues on `xminihub/upland` via the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Domain docs

Single-context: `CONTEXT.md` + `docs/adr/` at the repo root. See `docs/agents/domain.md`.

### Language

Write all markdown you generate for this repo — specs, issues, task files, `CONTEXT.md`, ADRs, reports — in **English**. Code, commands, file paths, identifiers, and quoted error messages stay verbatim. This applies to new output only; don't translate existing docs.
