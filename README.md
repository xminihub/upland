# upland

Upland is a Rust web application: an [axum](https://github.com/tokio-rs/axum) HTTP JSON API that also serves an embedded React single-page app (Vite, in `web/`).

## Endpoints

- `GET /api/hello` — hello message
- `GET /healthz` — health check
- `GET /` — single-page app (served from `web/dist`, embedded into the binary in release builds; when `web/dist` is empty the SPA routes are not mounted and the API works alone)

The server listens on `0.0.0.0:8080` by default.

## Development

Backend:

```sh
cargo run             # start the server on 0.0.0.0:8080
cargo test --locked     # unit tests + HTTP integration tests (axum-test, in-process)
cargo fmt --check     # format
cargo clippy --locked --all-targets  # lints (CI denies warnings)
```

Frontend (Bun, in `web/`):

```sh
cd web
bun install           # install dependencies (bun.lock committed)
bun run dev           # Vite dev server on :5173, proxies /api to 127.0.0.1:8080
bun run build         # typecheck + production build into web/dist (embedded by cargo build --release)
bun run lint          # oxlint (template default)
bun run test          # Vitest (jsdom, app-root seam)
bun run format:check  # Prettier check (CI enforces; bun run format to fix)
```

For full-stack development run `cargo run` and `bun run dev` side by side and open http://localhost:5173.

SPA-serving Rust tests skip when `web/dist` has not been built; run `bun run build` first to exercise them locally.

CI (`.github/workflows/ci.yml`) runs the Rust lint job, a frontend job (frozen install, lint, format check, test, build) and the test job — the test job consumes the frontend build output so SPA serving is tested for real.

## Releases

Releases are cut manually: dispatch the `Release` workflow from the Actions tab with a `patch`, `minor`, or `major` bump. It bumps the version with cargo-edit, commits and tags `v<version>`, builds binaries for linux-amd64, macos-arm64 and windows-amd64 (each matrix job builds the frontend first so the SPA is embedded via rust-embed), and publishes a GitHub Release with `upland-<os>-<arch>` archives. The single binary serves both API and UI; no separate frontend deployment.

## License

MIT — see [LICENSE](LICENSE).
