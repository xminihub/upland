# 6. Release channels: priority, the installer, and the cargo channel

Date: 2026-11-18

Implements: issue #27. Supersedes parts of [0001-distribution.md](0001-distribution.md).

## Context

ADR-0001 made Homebrew the documented channel and recorded, as a non-goal,
that "there is no installer script". Six weeks of releases say that decision
was wrong in one place and incomplete in two others.

Wrong: the org tap never onboarded upland. `tools/` holds `lim.json` only and
`Formula/` holds `lim.rb` only, so every bump the release dispatches dies at
the tap's first step, "no tools/upland.json in this tap". Releases v0.1.4 and
v0.1.5 (runs 44273610449 and 44304028258) each went green through
`github-release` and red at `tap`. The documented channel has never resolved:
`brew install upland` fails at the tap, and the release page is the only
thing that has ever shipped a byte. The pipeline's own gate — the formula
read-back — correctly refused to call that green.

Incomplete first: the binary channel, the thing that actually ships, cannot
be scripted. Asset names carry the version (`upland-<version>-<target>.tar.gz`)
because ADR-0001 chose that for republish visibility, and the consequence was
never followed: no stable download URL exists, so "install the binary" is
manual release-page archaeology with no checksum step and no platform
detection. ADR-0001 already half-saw it — "SHA256SUMS exists without an
installer to consume it."

Incomplete second: `cargo install upland` was neither adopted nor declined.
crates.io carries no `upland` (the API 404s; the name is free), and ADR-0001's
declination list covers installer, npm and container but not the registry, so
the absence reads as an oversight rather than a decision.

And priority was never expressed anywhere: a reader could not tell primary
from convenience from last resort.

## Decision

### Channel priority (supersedes 0001 §Channels and priority)

Three channels, in this documented order — the order users are pointed to
install in:

1. **Binary** — GitHub **release assets**, unchanged in shape, plus a new
   **installer** for Linux and macOS.
2. **Homebrew** — the **tap** keeps all four **brew targets** and the whole
   bump contract; documentation frames it as the macOS convenience channel.
3. **cargo** — the crate published to crates.io; `cargo install upland`.

Priority is install order only, not failure tolerance. Every channel gate
stays hard: a release is green when the assets, the formula and the crate all
land. The rejected reading of "binary first" was "binary alone is a gate" —
that would have quietly downgraded the tap and cargo checks to advisory.

### The installer (reverses 0001's rejection of `install.sh`)

`scripts/install.sh` — POSIX sh, so the stock shell of any Linux or macOS
host runs it — resolves a release, downloads this machine's **tarball
asset**, verifies it against the **checksum manifest**, and installs the
binary. Contract:

- Target detection maps `uname -s`/`uname -m` onto the **target table**'s
  npm-convention names (darwin/linux × arm64/x64); anything else — including
  Windows — is refused with a pointer to where its artifact lives. The
  detection and the asset-name pattern are re-spelled in the script rather
  than read from the table or `asset-name.sh`: the installer runs on a
  machine that has no repository yet, so byte-for-byte standalone shipping
  (the contract that makes the shown source the executed bytes) buys one
  deliberate vocabulary duplication. The script names the table as its
  source of truth; renaming a target there must be echoed here, and the
  tap's `render.mjs` keeps the same obligation for its half.
- Version: `UPLAND_VERSION` pins; otherwise one redirect hop on
  `releases/latest` names it, never an unauthenticated API call with its rate
  limit. The release-asset copy is versionless, so the pinned form is
  `releases/download/vX.Y.Z/install.sh` and the unpinned form is
  `releases/latest/download/install.sh`.
- Verification before anything moves: the manifest must name the asset and
  the sha256 must match; either refusal exits nonzero without installing.
- Installs to `~/.local/bin`, never elevated; `--bin-dir` overrides; warns
  when the destination is absent from `PATH`.

The package job copies the script byte-for-byte into every release as the
versionless asset `install.sh`. It is not listed in `SHA256SUMS` — the
manifest names the bytes a channel installs, and the script is transport, not
artifact; a manifest entry verifying the tool that reads the manifest is
circular — and it is not counted by the packer's asset-per-target guard.

**Verification of the installer is deliberately absent**: no CI job, no
release-time gate, no contract script. This is an explicit exemption from the
standard 0001 set for the packer ("assert the contract, don't ship a guess"):
the installer's failure mode is first-run-loud against real assets, and
simulating the release layout in a fixture is a second contract to maintain
for a script measured in tens of lines. The exemption was re-proposed and
re-confirmed in the design session that produced this ADR; the inconsistency
with the packer's treatment is chosen, not an oversight. Consequence, stated:
the script a user pipes is released by a workflow that never ran it; an edit
can reach a release broken, and the first discovery is that user's.

### The cargo channel (extends 0001; adds no reversal)

- The release publishes the crate as the **last job**: the chain
  `github-release → tap → cargo`, so the run graph reads as the channel
  order and the one-shot registry version is burned only after every higher
  channel is green.
- **Already-published is a skip with a notice, not a failure.** The
  registry's one-shot rule collides with this pipeline's repair contract
  (re-dispatch), so the job asks the sparse index for the version first.
  The asymmetry remains real: assets can be republished, a crate version
  cannot.
- The crate ships the **SPA bundle**: the job downloads the `web-dist`
  artifact verify already built — the same bytes every target embeds, per
  0001's one-build-feeds-all — and the manifest's `include` whitelist pulls
  the gitignored bundle into the package. The precedence (whitelist beats
  gitignore) was proven mechanically on the current tree, not assumed.
- **The publish commits the staged bundle** (`scripts/publish-crate.sh`), in
  the runner's checkout only, never pushed. Release run 44333475787 (v0.1.6)
  went green through the release and the tap and died here: cargo counts every
  packaged file git has not committed as an uncommitted change, and the
  whitelist packages `web/dist` whatever `.gitignore` says, so a clean tag
  checkout plus the artifact still refused to publish, on 24 bundle files.
  Committing the bundle keeps cargo's own clean-tree check as the guard;
  `--allow-dirty` would have waived it for every other file the registry
  receives. The fallback the draft of this ADR named for a precedence
  inversion — staging the bundle at a non-ignored path — would not have helped
  either: packaged-but-untracked files fail the same check, tested on the tag
  tree.
- **Packaging mistakes never burn a version**: verify gained a pre-bump
  gate asserting the packager's file listing carries `web/dist/index.html`,
  `migrations/` and `.sqlx/`, and the cargo job asserts the same listing
  again before publishing. The listing was never the whole contract — v0.1.6
  proved a correct listing can still refuse to publish — so CI's `packaging`
  merge job dry-runs the publish itself, the same script the cargo job runs,
  and a refusal of this kind now costs a red run instead of a half-released
  version.
- Auth is a publish-only API token in the `CARGO_REGISTRY_TOKEN` secret;
  a missing secret fails the job with a named reason.
- **No backfill**: `upland` enters the registry with the next CI release.
  0.1.4 and 0.1.5 are not hand-published; the registry will hold no version
  that CI has not proven.

The manifest also became publishable: SPDX `license` replacing the
license-file pointer, `rust-version` declared at the code's real floor
(1.87 — edition 2024 alone would claim 1.85, but `is_multiple_of` ships in
1.87 and the MSRV-aware lints hold the manifest to the truth), and
keywords/categories so the crate page and the resolver tell the truth about
the crate. (`homepage` was tried and dropped: cargo warns it is redundant
with `repository`, and CI denies warnings.)

### Tap onboarding (the missing half of 0001's contract)

Upland is seeded into the org tap by direct push to its main: `tools/upland.json`
(naming all four brew targets, per the table, and the MIT license) and
`templates/upland.rb.tpl` (a formula template whose service block runs the
server on loopback port 8310 with `keep_alive`, and whose caveats name the
two commands and the port's ownership — the shape 0001 §The brew service
already decided). No tap code changes: the existing `render.mjs` machinery
and bump gates do the rest.

Consequence: the `tap` job's failure — the one that reddened v0.1.4 and
v0.1.5 — has a cause and a fix; the first release after this lands is the
first that can go green with a formula behind it. The tap's daily backfill
may create `Formula/upland.rb` for an already-published release before that
next bump; that is the convergence it exists for, not drift.

## Consequences

- README's Install section is now ordered quickstarts (installer one-liner,
  brew with macOS framing, `cargo install upland`, Windows noted as manual),
  and `CONTEXT.md` names the **cargo channel** and the **installer**.
- A green release now means three channels landed; a red cargo job burns no
  registry version but does leave the higher channels published — the
  re-dispatch repair path (assets republish identically, bump re-lands,
  publish skips) is the recovery, and it is the reason skip-not-fail was
  chosen.
- The `include` whitelist makes crate contents an explicit list: a new
  path the crate needs (a second migrations tree, an assets folder) is a
  manifest edit, and the pre-bump listing gate is what catches forgetting.
- Adding a brew target to the table now also means adding it to the tap's
  `tools/upland.json` — 0001's two-file cost, newly payable because upland
  is now in that file.

## Rejected alternatives

- **Softening the gates** (best-effort brew/cargo): rejected — priority is
  how users install, not what may be broken. A green release continues to
  mean every channel landed.
- **Removing Linux from the brew targets**: rejected — the table and the tap
  contract stay as 0001 decided; only documentation framing moves.
- **Hand-publishing 0.1.4/0.1.5 to the registry**: rejected — an
  unprovenance version is exactly what the republish-pinned naming exists to
  avoid.
- **Trusted publishing (OIDC)**: rejected for now — the first publish of an
  unowned name through it was not verifiable from here, and a publish-only
  token is documented, immediate and rotatable.
- **An extracted, tested `publish-crate.sh` seam**: rejected — fewer seams
  beat one more contract; the skip branch is proven by the first repair
  re-dispatch, the same "proven at release" pattern 0001 established.
- **A UI-less crate**: rejected — the third channel would ship a knowingly
  degraded product, against 0001's own one-build-feeds-all principle.
- **A contract test for the installer**: rejected twice — in the design
  session's challenge round, over the packer's contrary precedent, and
  recorded above as the accepted risk it is.
