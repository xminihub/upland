# 1. Distribution: targets, artifacts, the org tap contract, the brew service

Date: 2026-11-18

## Context

upland had never been released. `Cargo.toml` said 0.1.0, no tag existed, no
GitHub Release existed, and `README.md` described archives nobody had ever
downloaded. So this is not a migration away from a broken channel: it is the
choice made before the first artifact exists, which is the only moment when the
naming is free.

What the workflow did produce, given a dispatch, was three archives —
`upland-linux-amd64.tar.gz`, `upland-macos-arm64.tar.gz`,
`upland-windows-amd64.zip` — each holding the binary plus `README.md` plus
`LICENSE`, with no version in the name. Against `xminihub/limulin` issue #3 and
the tap it built (`xminihub/homebrew-tap`), that shape is wrong four ways: the
tap's contract is `<bin>-<version>-<target>.tar.gz` with one member, its
`bump.yml` refuses to render a target whose asset is missing, it wants four
brew targets rather than three os/arch labels, and a formula pinning an
un-versioned asset cannot tell a republished version from the original.

The sibling pattern was available to copy: limulin renders a Ruby template in
its own repository and pushes it into a per-product tap, which turned out to
have never published a formula, pinned one target out of five, and needed a
PAT with content-write access to another repository. Its issue #3 replaced all
of that with per-target tarball assets plus a dispatch to the org tap, and
`oh-my-harness` and `luminous-rs` are queued behind it. Copying only the
conclusion would have copied the wrong parts too: upland is a **server**, and
limulin's formula shape — `bin.install`, a `--version` test, nothing else —
describes a command a person types.

Two facts about this program shaped the rest. `src/main.rs` read no arguments
and hardcoded `0.0.0.0:8080`, so a brew-installed upland would either need a
human to start it in the foreground or open a port on their network the moment
`brew services start upland` ran (ADR-0002 decides the defaults; this document
decides the artifacts). And Homebrew on Linux simply downloads whatever bytes
we upload: a `x86_64-unknown-linux-gnu` build made on Ubuntu 24.04 requires
glibc 2.39, and `brew fetch --all-platforms` — the tap's strongest gate —
verifies a checksum, never that a binary starts on the user's libc.

## Decision

### Channels and priority

- Homebrew is the documented channel: `brew tap xminihub/tap`, then
  `brew install upland`. Raw **release assets** are the fallback for anything
  Homebrew does not cover.
- There is no installer script, no npm package and no container image. They are
  declined below rather than left unwritten.

### Targets and artifacts

- The **target** vocabulary is the npm convention everywhere: `darwin-arm64`,
  `darwin-x64`, `linux-x64`, `linux-arm64`, `win32-x64`. Nothing publishes
  `macos-arm64`, `linux-amd64`, `windows-amd64` or `linux-aarch64`.
- `scripts/targets.json` is the **target table**: five rows carrying each
  target's rust target triple, runner label, macOS deployment floor, brew
  membership and executable suffix. `release.yml`'s build matrix is that file
  (`fromJSON`), and `scripts/asset-name.sh` is the only place an asset name is
  spelled. A target that exists in the table and not in a workflow, or the
  other way round, is now a single-file mistake.
- One artifact per target is published: a **tarball asset** for each of the four
  **brew targets**, and `upland-<version>-win32-x64.exe` for the fifth, plus
  `SHA256SUMS`. The tarball's only member is the binary renamed to `upland`.
  `README.md` and `LICENSE` are not inside any artifact — the release page and
  the repository carry them, and a second member in the archive is a second
  thing for `bin.install` to be wrong about.
- Linux ships **static musl** (`x86_64-unknown-linux-musl`,
  `aarch64-unknown-linux-musl`) under the plain `linux-x64`/`linux-arm64`
  names. The tap's contract is about the name, not the libc inside it, and the
  name the tap renders a URL for is the one an Ubuntu 20.04 or Alpine host can
  actually start. No glibc **release asset** is published beside it.
- `darwin-x64` builds with `MACOSX_DEPLOYMENT_TARGET=11.0`; `darwin-arm64`
  keeps 14.0. The Intel floor is the point of having the target at all.
- Each build job runs the binary it just produced with `--version` and compares
  the output to the version being released, before packaging. `x86_64` macOS
  binaries are executed under Rosetta, which that job installs.
- **Republishing a version is legal**: the packer pins the USTAR header (mode
  `0755`, uid/gid `0`, mtime at the epoch) and gzip's own name/mtime fields, so
  unchanged bytes hash identically and a re-upload cannot leave
  `Formula/upland.rb` disagreeing with its asset.

### The packer

- `scripts/package-brew-tarball.sh` writes those archives with GNU tar, on
  Linux, in one place: `tar --format=ustar --owner=0 --group=0
  --numeric-owner --mode=0755 --mtime=@0 --sort=name | gzip -n -9`.
- It refuses a tar that is not GNU tar instead of quietly writing bsdtar's
  defaults, and names `brew install gnu-tar` as the way to run it on macOS.
- `scripts/test-package-brew-tarball.sh` asserts the header fields, the payload,
  the reported sha256, byte-for-byte equality across two runs of the same
  input, the name derivation for every row of the table, and the refusals. It
  is `ci.yml`'s `packaging` job, deliberately not a `cargo test`: the
  assertions are about GNU tar's output, and the two tar dialects differ in
  exactly the fields under test.
- Packaging is shell plus `jq` plus Python's `tarfile`. limulin's equivalent is
  a Bun/TypeScript script because that repository *is* Bun; adding a Node
  runtime and a root `package.json` to a Rust repository to gain one
  deterministic tarball writer would have made the frontend toolchain a second
  build dependency of the release.

### The release pipeline

- `verify` runs every gate — `cargo fmt --check`, `cargo clippy --locked
  --all-targets` with `-Dwarnings`, `cargo test --locked`, and the frontend's
  lint, format check, tests and build — before `release` bumps, commits or
  pushes a tag. A red release leaves no version to explain.
- `verify` also publishes the `web-dist` artifact that every target build
  downloads, so the four **tarball assets** of one release embed identical
  frontend bytes and `bun install` runs once per release instead of once per
  target.
- `package` (Ubuntu, GNU tar) renames, packs and checksums; `github-release`
  uploads exactly what `package` produced; `tap` then dispatches the bump and
  polls it. A failed bump fails the release.
- The credentials are split by direction: the tap commits with its own token,
  and this repository dispatches with `TAP_TRIGGER_TOKEN`, an Actions-only
  fine-grained PAT. `HOMEBREW_TAP_TOKEN` — a content-write PAT for another
  repository that no workflow in this one ever referenced — is deleted.

### The tap contract

- One tap for the org: `brew tap xminihub/tap`, backed by
  `xminihub/homebrew-tap`. No `homebrew-upland` repository is created.
- The tap owns `Formula/upland.rb`, `tools/upland.json` and
  `templates/upland.rb.tpl`. Nothing in this repository renders Ruby, holds a
  formula template, or knows what a `sha256` of one of its own assets should
  be — the tap computes those from the published files.
- Asset naming is the contract between the two repositories, and it was checked
  mechanically rather than assumed: `node scripts/render.mjs assets upland
  0.1.1` in the tap and `bash scripts/asset-name.sh --version 0.1.1 --target
  <t> --kind tarball` here print the same four names.
- `Formula/upland.rb` is seeded by the first release through this path, not
  committed by hand in either repository.

### macOS signing

- No code signing, no notarization, no Apple credentials in this repository.
  limulin signs because a `bun build --compile` output `dlopen`s an
  ad-hoc-signed dylib and needs two entitlements to survive the hardened
  runtime it asked for; a Rust binary here has no equivalent problem, and
  Homebrew's download path sets no quarantine xattr, so there is no Gatekeeper
  assessment to satisfy in advance.
- Consequence, stated rather than discovered later: a **release asset** fetched
  by a browser carries the quarantine xattr and is assessed on first run. If
  that ever becomes the complaint, signing the two darwin targets is the fix —
  and it is a decision to reopen here, not a config to guess at.

### The brew service

- `templates/upland.rb.tpl` declares a `service` block running
  `[opt_bin/"upland", "--host", "127.0.0.1", "--port", "8310"]` with
  `keep_alive true`, logs under `var/log/upland.log`, and `caveats` that name
  the two commands and say where the port lives.
- The port is the tap's number to own; this repository's source never mentions
  it. `--host`/`--port` (ADR-0002) exist so that a package manager can pick an
  address without a config file format being invented first, and 8310 is chosen
  so that starting the service cannot collide with the 8080 a contributor's
  `cargo run` is already holding.
- Changing it means editing the generated plist and restarting: `brew services`
  passes no arguments, and a reinstall or restart writes 8310 back. That is
  documented in the caveats because it is otherwise undiscoverable.

## Consequences

- What is proven and what is not, at the time of writing: the CLI surface, the
  name derivation, the packer's refusals and the tap's rendering of upland's
  four targets are verified locally (`cargo test`, the packaging assertions'
  logic against fixture archives, `node scripts/render.mjs` plus `brew style`
  on a rendered formula with no offenses). `brew fetch --all-platforms`,
  `brew audit --strict --online`, `brew install` and `brew test upland` cannot
  pass until assets exist — they are the first release's proof, not this
  document's.
- Nothing breaks for existing users, because there are none: no tag, no release
  and no download of `upland-<os>-<arch>.tar.gz` ever existed. `README.md`
  documented a naming scheme that was never published, which is why changing it
  cost nothing.
- One build now feeds four tarballs from one Linux host, so a musl link failure
  shows up as a red **build** job on the target that hit it, and the answer is
  `cargo zigbuild`, never a quiet switch back to gnu.
- `linux-arm64` builds on `ubuntu-24.04-arm`, a GitHub-hosted arm64 runner:
  free for public repositories, native, so no cross linker and no container
  toolchain in the release path.
- A release is now longer than the sum of its builds: the tap's bump runs on a
  macOS runner because `brew` is not on the PATH of Ubuntu ones, and the
  release waits for it. A tap-side push failure stops a release, and is
  repaired by re-dispatching `bump.yml` for the same version.
- Adding a target means one row in `scripts/targets.json`, and nothing else in
  this repository — unless it is a brew target, in which case the tap's
  `tools/upland.json` has to name it too, and a bump that finds no such asset
  fails by name.
- `SHA256SUMS` exists without an installer to consume it. It is published
  because a checksum manifest that names the exact bytes a channel installed is
  the difference between an auditable release and a directory of files.
- Two files in another repository (`tools/upland.json`,
  `templates/upland.rb.tpl`) now gate this repository's release. That is the
  same trade the org tap was created to make, and the daily `backfill.yml` in
  that repository converges a formula this repository's trigger failed to bump.

## Rejected alternatives

- **A per-project `homebrew-upland` tap, or a formula rendered here.** Exactly
  what limulin's issue #3 removed: a template in the source repository that had
  never published a formula, pinned one of five targets, and needed a
  content-write credential for a second repository.
- **Keeping `upland-<os>-<arch>.tar.gz`.** The tap cannot render it, and an
  un-versioned asset makes republishing invisible: two different files with the
  same name and no way to say which one a formula pinned.
- **Publishing bare binaries and tarballs, as limulin does.** Nine assets per
  release where five carry the same bytes. Limulin needs both because its
  `install.sh` downloads the bare binary and its formula pins the tarball;
  upland has no installer, so the tarball is the only thing anyone would
  download.
- **`x86_64-unknown-linux-gnu` built on Ubuntu 24.04** (a glibc 2.39 floor), or
  on `ubuntu-22.04` (2.35). Both are cheaper to build and both refuse to start
  on hosts the tap's own users have.
- **A musl asset under a `linux-x64-musl` name beside the gnu one.** The tap's
  `render.mjs` cannot render a target outside `os-cpu` from that vocabulary, so
  the extra name would be a channel with no formula — and a formula that
  silently skipped it is how `linux/arm64` went missing from a sibling tap.
- **`cross` with docker for linux-arm64.** Reproducible, and a musl sweep would
  be easier later, but it puts a docker daemon in the release path when GitHub
  ships a native arm64 runner for public repositories.
- **A Bun/TypeScript packer copied from limulin.** Byte-determinism was the
  property worth copying, not the runtime: it costs a root `package.json`, a
  lockfile and a Bun toolchain in a repository whose only TypeScript is a
  frontend build.
- **A `cargo test` for the packer.** Its assertions are about GNU tar's output;
  a developer's `tar` is bsdtar, so the test would pass or fail depending on
  whose laptop ran it.
- **Signing and notarizing the darwin targets for org parity.** Limulin's
  entitlements problem is Bun's, not ours; this buys two Apple credentials, a
  keychain dance and a 30-minute notarization step for a program Homebrew
  downloads without a quarantine xattr.
- **A `caveats`-free formula, or the service port left to the binary's default.**
  A server installed with `brew install upland` and then started would either
  collide with the developer's 8080 or bind `0.0.0.0`; the caveats are the only
  place a person learns that the thing they installed is a service.
- **A config file format for the service** (a `var/upland/config` the binary
  reads). Nothing in the persistence design needs one yet, and a format is a
  promise; `--host`/`--port` plus the plist cover the override.

## Explicitly out of scope

- `install.sh`, an npm channel, a container image, winget and scoop: recorded
  here as non-goals so that `SHA256SUMS` does not read as a half-written
  installer.
- Bottles, `brew test-bot`, and submitting to `homebrew-core`.
- Windows beyond the published `.exe`: Homebrew has no Windows, and no
  Windows-specific packaging was requested.
- Anything in `xminihub/homebrew-tap` beyond `tools/upland.json` and
  `templates/upland.rb.tpl` — the tap's generator, gates and README are
  unchanged, which is the point of the two-file cost.
- Graceful shutdown, signal handling and any other **server** behaviour beyond
  choosing a **listen address**.
