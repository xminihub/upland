#!/usr/bin/env bash
# The target table is the matrix: one file decides which targets exist, where
# each is built and what its artifacts are called (ADR-0001). This is that
# table's contract in one exit code — a row that cannot become a job is a
# broken table, not a partial release. release.yml's verify job runs it as
# its first step, so a bad row costs seconds instead of toolchain minutes.
#
#   scripts/check-targets.sh
#     -> the compact targets array on stdout: the build matrix, fed straight
#        into GITHUB_OUTPUT
#
# The target names are listed on stderr, so the run's log still reads as
# names, not JSON.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
table="$script_dir/targets.json"

if [ "$#" -gt 0 ]; then
    echo "usage: check-targets.sh (takes no arguments)" >&2
    exit 2
fi

if ! command -v jq >/dev/null 2>&1; then
    echo "check-targets: jq is required" >&2
    exit 1
fi

require() { # <jq program> <why the table is broken>
    if ! jq -e "$1" "$table" >/dev/null; then
        echo "check-targets: $2" >&2
        exit 1
    fi
}

require '.bin == "upland"' \
    '.bin must name the binary: upland'
require '.targets | length > 0' \
    'the table holds no targets'
require 'all(.targets[];
    .name and .rustTarget and .runner and (.brew != null) and (.exe != null))' \
    'a target is missing name, rustTarget, runner, brew or exe'
require '(.targets | map(.name) | length) == (.targets | map(.name) | unique | length)' \
    'target names are not unique'
require '[.targets[] | select(.brew)] | length > 0' \
    'no brew target: the org tap would pin nothing'
# A brew target the org tap cannot render fails its bump by name; the
# os/cpu halves are checked here so a bad row costs a minute, not a
# published release.
require 'all(.targets[] | select(.brew);
    (.name | split("-")) as [$os, $cpu] |
    ($os == "darwin" or $os == "linux") and ($cpu == "arm64" or $cpu == "x64"))' \
    'a brew target is not <darwin|linux>-<arm64|x64>: the org tap cannot render it'

jq -r '.targets[].name' "$table" >&2
jq -c '.targets' "$table"
