# upland

Upland is a Rust HTTP JSON API built with [axum](https://github.com/tokio-rs/axum).

## Endpoints

- `GET /` — hello message
- `GET /healthz` — health check

The server listens on `0.0.0.0:8080` by default.

## Development

```sh
cargo run             # start the server on 0.0.0.0:8080
cargo test            # unit tests + HTTP integration tests (axum-test, in-process)
cargo fmt             # format
cargo clippy --all-targets  # lints (CI denies warnings)
```

CI (`.github/workflows/ci.yml`) runs lint (fmt/check/clippy) and tests on every push to `main`.

## Releases

Releases are cut manually: dispatch the `Release` workflow from the Actions tab with a `patch`, `minor`, or `major` bump. It bumps the version with cargo-edit, commits and tags `v<version>`, builds binaries for linux-amd64, macos-arm64 and windows-amd64, and publishes a GitHub Release with `upland-<os>-<arch>` archives.

## License

MIT — see [LICENSE](LICENSE).
