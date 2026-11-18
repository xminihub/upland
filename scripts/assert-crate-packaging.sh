#!/usr/bin/env bash
# The crate-packaging assertion (ADR-0006), in one place because two call
# sites run it: `verify` executes it before the bump so a packaging mistake
# leaves no version to explain, and the `cargo` job executes it again before
# the one-shot publish so the registry never holds a half-packed crate.
#
# It asserts the packager's file listing — what would ship — not that the
# crate builds; the publish's own verify-build covers that. Precondition:
# the release's web-dist is staged at web/dist, exactly as the jobs do.
set -euo pipefail

listing="$(cargo package --list --allow-dirty)"
for want in web/dist/index.html migrations/ .sqlx/; do
  printf '%s\n' "$listing" | grep -qF "$want" || {
    echo "the packaged crate does not carry $want"
    exit 1
  }
done
echo "crate packaging asserted: $(printf '%s\n' "$listing" | wc -l) files packaged"
