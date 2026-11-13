# AGENTS.md

## Development

- `cargo fmt` — format (CI enforces `cargo fmt --check`)
- `cargo clippy --locked --all-targets` — lints; CI runs with `RUSTFLAGS: -Dwarnings`, so warnings fail the build
- `cargo test --locked` — full suite (unit + HTTP integration tests)
- `cargo run` — start the API server on `0.0.0.0:8080`

Frontend (`web/`, Bun; CI pins bun 1.4.4):

- `bun install` — install (lockfile `web/bun.lock` is committed; use `--frozen-lockfile` in CI)
- `bun run lint` — oxlint; `bun run format:check` — Prettier (fix with `bun run format`); both enforced in CI
- `bun run build` — typecheck + production build into `web/dist`
- `bun run dev` — Vite dev server on :5173, proxies `/api` to 127.0.0.1:8080

All HTTP behavior is tested through the in-process router seam (see `tests/`); tests must not open real sockets. SPA-serving tests skip when `web/dist` is empty; run `bun run build` first to exercise them locally (the CI test job consumes the frontend job's build artifact).

## Agent skills

### Issue tracker

Issues are tracked as GitHub Issues on `xminihub/upland` via the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Domain docs

Single-context: `CONTEXT.md` + `docs/adr/` at the repo root. See `docs/agents/domain.md`.

### Language

Write all markdown you generate for this repo — specs, issues, task files, `CONTEXT.md`, ADRs, reports — in **English**. Code, commands, file paths, identifiers, and quoted error messages stay verbatim. This applies to new output only; don't translate existing docs.
