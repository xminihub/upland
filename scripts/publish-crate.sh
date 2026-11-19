#!/usr/bin/env bash
# The cargo channel's publish (ADR-0006), in one place because two call sites
# run it: the release's `cargo` job publishes for real, and CI's `packaging`
# merge job dry-runs the same path. That dry run is what makes a publish that
# would refuse cost a red run instead of a version stuck half-released — the
# cargo job is the pipeline's last step, on a tag that already exists, with a
# registry version that can never be re-cut.
#
# Precondition: the built SPA bundle is staged at web/dist and the crate
# listing is already asserted (assert-crate-packaging.sh); this script runs
# after that, and does not re-check the listing.
#
# Usage:
#   CARGO_REGISTRY_TOKEN=… scripts/publish-crate.sh <version>   # publish
#   scripts/publish-crate.sh --dry-run                          # prove it would
set -euo pipefail

dry_run=0
if [ "${1:-}" = "--dry-run" ]; then
  dry_run=1
  shift
fi
version="${1:-}"
if [ "$dry_run" = 0 ] && [ -z "$version" ]; then
  echo "usage: scripts/publish-crate.sh [--dry-run] <version>" >&2
  exit 2
fi

if [ "$dry_run" = 0 ] && [ -z "${CARGO_REGISTRY_TOKEN:-}" ]; then
  echo "::error::the CARGO_REGISTRY_TOKEN secret is not configured; the cargo channel cannot publish"
  exit 1
fi

# Already-published is a skip, not a failure: this pipeline's repair contract
# is re-dispatch, and a re-dispatch cannot un- or re-publish a registry
# version. The sparse index answers; a missing crate answers as
# nothing-published, which is the first release. A dry run never uploads, so
# it asks nothing of the registry and always runs.
# (jq without -e: `-e` exits 1 when the test is false, which `set -e` here
# would take as the very failure it is testing for.)
if [ "$dry_run" = 0 ]; then
  index="$(curl -fsS https://index.crates.io/up/la/upland || true)"
  published="$(printf '%s' "$index" | jq -s --arg v "$version" 'any(.[]; .vers == $v)')"
  if [ "$published" = "true" ]; then
    echo "::notice::upland $version is already on crates.io — publish skipped"
    exit 0
  fi
fi

# Precondition, asserted rather than assumed: without the staged bundle the
# crate ships build.rs's empty embed folder, and `git add` below would fail on
# a path that is not there with a message that says nothing about the artifact.
[ -f web/dist/index.html ] || {
  echo "web/dist does not carry the built SPA bundle: stage the web-dist artifact first (assert-crate-packaging.sh expects it too)" >&2
  exit 1
}

# Commit the bundle before publishing — in this checkout only, never pushed.
# No --allow-dirty is passed anywhere: a published version is forever, and the
# registry must only ever hold what a clean tag checkout produced. But cargo's
# clean-tree check counts every packaged file git has not committed, and the
# manifest's include whitelist packages web/dist whether or not .gitignore
# covers it, so an untouched tag plus the staged bundle reads as a dirty tree —
# run 44333475787 refused 24 web/dist files on exactly that. Committing keeps
# cargo's own check as the guard instead of waiving it: drift in any other
# shipped file still stops the publish, and the commit records what the
# registry received.
git add -fA -- web/dist
# Committed once is enough: a tree that already carries the bundle — a local
# second run, say — has nothing to add, and an empty commit would fail.
if ! git diff --cached --quiet -- web/dist; then
  git -c user.name="github-actions[bot]" \
      -c user.email="41898282+github-actions[bot]@users.noreply.github.com" \
      commit -q -m "release: stage the web bundle for the cargo channel"
fi

if [ "$dry_run" = 1 ]; then
  # --no-verify: CI's Rust lane already built and tested this tree; the point
  # here is the packager's checks — the manifest, the listing, the clean tree.
  exec cargo publish --locked --dry-run --no-verify
fi

exec cargo publish --locked
