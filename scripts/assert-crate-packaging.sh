#!/usr/bin/env bash
# The crate-packaging assertion (ADR-0006), in one place because three call
# sites run it: CI's `packaging` merge job executes it first, so a packaging
# mistake costs a red run instead of hiding until release day; `verify`
# executes it before the bump so a red release leaves no version to explain;
# and the `cargo` job executes it again before the one-shot publish so the
# registry never holds a half-packed crate.
#
# It asserts the packer's file listing — what would ship — not that the
# crate builds; the publish's own verify-build covers that. Precondition:
# the built SPA bundle is staged at web/dist, exactly as the jobs do.
set -euo pipefail

listing="$(cargo package --list --allow-dirty)"
for want in web/dist/index.html migrations/ .sqlx/; do
  printf '%s\n' "$listing" | grep -qF "$want" || {
    echo "the packaged crate does not carry $want"
    exit 1
  }
done
echo "crate packaging asserted: $(printf '%s\n' "$listing" | wc -l) files packaged"
