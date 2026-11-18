# CONTEXT

Domain language for upland. This is a glossary, not a spec: names here are the
names code, issues, tests, workflows and ADRs must use.

## Product

- **upland** — the project, the repository and the command users type. One
  binary: it serves the JSON API and the **SPA bundle** from the same process.
- **server** — what `upland` does when it runs: bind a **listen address**, serve
  until interrupted. Nothing else in this repository's vocabulary calls it an
  app, a CLI or a tool.
- **listen address** — the `host:port` the binary binds, from `--host`/`--port`
  or `UPLAND_HOST`/`UPLAND_PORT`, defaulting to `127.0.0.1:8080`
  (ADR-0002). `localhost` is not the term: the flag takes an address, and
  `0.0.0.0` is a legal one. An IPv6 host is bracketed when the address is
  composed, because `::1:8080` is itself a legal IPv6 literal that means
  something else.
- **SPA bundle** — the Vite build in `web/dist`, embedded into the binary by
  `rust-embed` at release compile time. A target's binary and its bundle are
  one artifact; there is no separate frontend deployment.

## Persistence

- **database** — the single SQLite file upland opens at startup, through
  SQLx with one writer connection and a read pool (ADR-0003). It is opened
  before the **listen address** is bound and fails the start if it cannot
  be opened. **store**, **datastore** and **cache** are not the term.
- **database path** — the location of the **database**: `--db-path` or
  `UPLAND_DB_PATH`, defaulting to the platform's per-user data directory
  (`upland/upland.db`), never the working directory (ADR-0003). The
  gateway's first tables — credentials, logs, analytics, sessions — land
  under it in their own migrations when the features that need them arrive.

## Artifacts

- **target** — one of `darwin-arm64`, `darwin-x64`, `linux-x64`, `linux-arm64`,
  `win32-x64`: the npm convention (ADR-0001). `linux-aarch64`, `macos-arm64`,
  `linux-amd64` and `windows-amd64` are retired names; nothing publishes them.
- **target table** — `scripts/targets.json`: the single source of the target
  list, each target's rust target triple, runner label, macOS deployment floor,
  brew membership and executable suffix. The release matrix, every asset name
  and the packer all read it; nothing spells a target out beside it.
- **binary** — `cargo build --release` output for one target. On Linux it is
  statically linked against musl, so a **tarball asset** of it runs on any
  glibc age. A binary is a build intermediate: only `win32-x64`'s is published
  as a **release asset**.
- **tarball asset** — `upland-<version>-<target>.tar.gz`, whose only member is
  the binary renamed to `upland`, mode `0755`, uid/gid `0` and mtime at the
  epoch. Built by `scripts/package-brew-tarball.sh`, which pins those header
  fields with GNU tar so the same bytes always hash to the same sha256 — the
  property that makes **republishing** legal.
- **brew target** — the **target** subset a formula covers: the four non-Windows
  targets. `win32-x64` is never a brew target; Homebrew has no Windows.
- **release asset** — a file attached to a GitHub Release: one **tarball asset**
  per **brew target**, `upland-<version>-win32-x64.exe`, and `SHA256SUMS`.
- **checksum manifest** — `SHA256SUMS`, listing the sha256 of every published
  asset, sorted by file name, as `<hash>  <file>` lines. It names the exact
  bytes a channel installed.
- **asset name** — derived only by `scripts/asset-name.sh` from the **target
  table**. This is the cross-repo contract with the **tap**: the tap's
  `scripts/render.mjs` pins URLs built from the same pattern, and a release
  that stops publishing one of them fails the **bump** rather than handing a
  user a 404.
- **republishing** — uploading the same version's assets again. Legal here,
  because a **tarball asset** of unchanged bytes hashes identically; it is not
  legal for an unsigned, un-versioned archive name.

## Channels

- **channel** — a way to install and run upland. **Homebrew** is the documented
  one; a **release asset** downloaded by hand is the other. There is no
  installer script, no npm package and no container image (ADR-0001).
- **tap** — what Homebrew calls `xminihub/tap`, i.e. `brew tap xminihub/tap`.
  Its Git repository is not this one: it is `xminihub/homebrew-tap`, the
  org-wide tap shared by every xminihub tool. upland has never owned a tap of
  its own.
- **formula** — `Formula/upland.rb` in the tap. Generated, never hand-written
  here: the source of truth is `tools/upland.json` plus
  `templates/upland.rb.tpl` in the tap, rendered and verified by that
  repository's `bump.yml`. This repository publishes the assets it pins and
  nothing else.
- **bump** — the tap's `bump.yml` run for one tool and version: it checksums the
  published assets, renders and gates the formula, and commits it. This
  repository's release dispatches the bump and polls it to its conclusion, so a
  green release means an installable formula.
- **brew service** — the launchd/systemd job `templates/upland.rb.tpl` declares:
  `brew services start upland` runs the **server** on loopback port 8310. That
  port belongs to the tap: no code, script or workflow here mentions it, and
  README quotes it only as the tap's choice. The **listen address** flags are
  what make it possible for the tap to choose one.

## Packaging vocabulary

- **packer** — `scripts/package-brew-tarball.sh`. Needs GNU tar; macOS ships
  bsdtar, whose header defaults differ in exactly the pinned fields, so the
  script refuses a non-GNU tar instead of writing a different archive.
- **packaging assertions** — `scripts/test-package-brew-tarball.sh`, run by
  `ci.yml`'s `packaging` job on Linux. Deliberately not a `cargo test`, for the
  same reason the packer refuses bsdtar.
