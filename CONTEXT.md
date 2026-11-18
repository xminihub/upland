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
  gateway's first tables — credentials, logs, analytics — land under it in
  their own migrations when the features that need them arrive.

## Authentication

- **account** — the person a request authenticates as: a username and an
  email, both stored lowercased and unique. Created at signup or, through
  an OIDC **provider**, silently at first login. **user** is not the term
  in code and issues; **principal** never was.
- **credential** — an account's bcrypt hash row, the one place a password
  is kept; an account that arrived by OIDC alone has none. The reserved
  table of ADR-0003 keeps this exact meaning.
- **identity** — an (issuer, subject) pair an OIDC login arrived with,
  attached to exactly one account. **provider name** is how the login
  screen and the API refer to it.
- **access token** — the stateless HS256 JWT a signed-in caller sends as
  `Authorization: Bearer`; nothing else opens the API (ADR-0004). It
  expires; it is never refreshed; logout forgets one rather than revoking
  it. **session** is not the term and there is no sessions table.
- **signing secret** — the random key in the **config file** that mints
  and verifies **access token**s; generated at first boot, owned by the
  operator from then on, and replacing it revokes every token.
- **provider** — an OIDC provider the operator configured: a name
  (`[a-z0-9-]`, quoted forever in payloads), an issuer URL and a client id.
  No client secret is stored server-side.
- **config file** — the JSON document holding everything the server reads
  at boot beyond the CLI surface (ADR-0005): the `server` and `auth`
  sections, generated with safe defaults on first boot. **store** and
  **settings** are not the term.
- **config path** — the location of the **config file**: `--config` or
  `UPLAND_CONFIG`, defaulting to `upland/config.json` in the same per-user
  data directory as the **database path**.

## Artifacts

- **target** — one of `darwin-arm64`, `darwin-x64`, `linux-x64`, `linux-arm64`,
  `win32-x64`: the npm convention (ADR-0001). `linux-aarch64`, `macos-arm64`,
  `linux-amd64` and `windows-amd64` are retired names; nothing publishes them.
- **target table** — `scripts/targets.json`: the single source of the target
  list, each target's rust target triple, runner label, macOS deployment floor,
  brew membership and executable suffix. The release matrix, every asset name
  and the packer all read it; nothing that *can* read it spells a target out
  beside it. The **installer** is the one exception that cannot — it runs
  before this repository exists on the user's machine — so it hand-spells the
  names and carries a comment pointing back here (ADR-0006).
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
  per **brew target**, `upland-<version>-win32-x64.exe`, `SHA256SUMS`, and the
  **installer**'s byte-identical versionless copy `install.sh` — transport,
  not artifact, so it is the one release asset the **checksum manifest** does
  not name.
- **checksum manifest** — `SHA256SUMS`, listing the sha256 of every published
  asset, sorted by file name, as `<hash>  <file>` lines. It names the exact
  bytes a channel installed.
- **asset name** — spelled in code only by `scripts/asset-name.sh`, from the
  **target table**. The **installer** builds the same name inline because it
  runs on a user's machine with no repository to read; that duplication is
  deliberate and ADR-0006 records it. This is the cross-repo contract with the
  **tap**: the tap's `scripts/render.mjs` pins URLs built from the same
  pattern, and a release that stops publishing one of them fails the **bump**
  rather than handing a user a 404.
- **republishing** — uploading the same version's assets again. Legal here,
  because a **tarball asset** of unchanged bytes hashes identically; it is not
  legal for an unsigned, un-versioned archive name.

## Channels

- **channel** — a way to install and run upland. Three, documented in this
  order (ADR-0006): the **binary channel** first — the **installer** on Linux
  and macOS, the **release assets** everywhere including Windows;
  **Homebrew** second, framed for macOS although the **formula** covers every
  **brew target**; the **cargo channel** third. The order is install order,
  not leniency: every channel gate is hard. There is still no npm package and
  no container image (ADR-0001).
- **binary channel** — the GitHub **release assets** and the **installer**
  that scripts them: the primary channel, and the only one that was ever
  actually published before ADR-0006.
- **installer** — `scripts/install.sh`: the POSIX **sh** program that maps
  `uname` onto a **target**, resolves a release (pinned by `UPLAND_VERSION`,
  else `latest` via one redirect), verifies the **tarball asset** against the
  **checksum manifest**, and installs to `~/.local/bin` without elevating.
  Attached to every release verbatim as the versionless `install.sh` asset —
  the stable URL beside version-named artifacts. Deliberately ungated by CI
  (ADR-0006): read it before piping it; that is its contract.
- **cargo channel** — `cargo install upland`: the **crate** published to
  crates.io as the release's last job. The crate carries the **SPA bundle**
  (the same `web-dist` build every other artifact embeds), the migrations,
  and the offline query metadata, so it compiles with no bun, node, or
  database service. Registry versions are one-shot: unlike **republishing**
  an asset, a published version is forever; the pipeline answers a
  re-dispatch with skip, never fail.
- **crate** — the package `cargo publish` sends to crates.io: sources,
  migrations, offline query metadata, and the **SPA bundle** staged by the
  same release that shipped it. Distinct from the **binary**: the crate is
  the program as source, built on the user's machine, published under a
  version that cannot be republished.
- **tap** — what Homebrew calls `xminihub/tap`, i.e. `brew tap xminihub/tap`.
  Its Git repository is not this one: it is `xminihub/homebrew-tap`, the
  org-wide tap shared by every xminihub tool. upland has never owned a tap of
  its own. Registration in the tap lives in two seed files there
  (`tools/upland.json`, `templates/upland.rb.tpl`), pushed when ADR-0006
  onboarded upland — until then every release reddened at the **bump**.
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
