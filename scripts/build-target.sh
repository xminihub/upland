#!/usr/bin/env bash
# Build and smoke one target exactly as its row in the target table says:
# zigbuild for the -musl triples (their C toolchain is installed by
# install-musl-toolchain.sh before this runs — the Ubuntu runners have no
# musl gcc, ADR-0001), plain cargo otherwise, and the row's deployment
# target for the darwin slices.
#
# Then the binary is run with --version and compared to the version being
# released, before anything can package it (ADR-0001): a build that produces
# an unrunnable artifact, or one that reports another version, is a red job
# rather than a shipped binary. On the x86_64 macOS row that proof happens
# under Rosetta, which the workflow has installed.
#
#   scripts/build-target.sh --target <name> --version <semver>
#
# Run it from the repository root: the build output lives under ./target/.
# The same call serves the Windows runner under Git Bash — the .exe half of
# the output path is a fact of the table, not of the platform.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
table="$script_dir/targets.json"

target=""
version=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        --target)
            target="${2:-}"
            shift 2
            ;;
        --version)
            version="${2:-}"
            shift 2
            ;;
        *)
            echo "build-target: unexpected argument: $1" >&2
            exit 2
            ;;
    esac
done

if [ -z "$target" ] || [ -z "$version" ]; then
    echo "usage: build-target.sh --target <name> --version <semver>" >&2
    exit 2
fi

if ! command -v jq >/dev/null 2>&1; then
    echo "build-target: jq is required" >&2
    exit 1
fi

row="$(jq -c --arg target "$target" '.targets[] | select(.name == $target)' "$table")"
if [ -z "$row" ]; then
    echo "build-target: not a target: $target (expected $(jq -r '[.targets[].name] | join(", ")' "$table"))" >&2
    exit 1
fi

rust_target="$(jq -r '.rustTarget' <<<"$row")"
deployment="$(jq -r '.deploymentTarget // ""' <<<"$row")"
if [ "$(jq -r '.exe' <<<"$row")" = "true" ]; then
    output="./target/$rust_target/release/upland.exe"
    exists() { [ -f "$1" ]; }
    missing="missing"
else
    output="./target/$rust_target/release/upland"
    exists() { [ -x "$1" ]; }
    missing="missing or not executable"
fi

case "$rust_target" in
    *-musl) set -- cargo zigbuild ;;
    *) set -- cargo build ;;
esac
if [ -n "$deployment" ]; then
    export MACOSX_DEPLOYMENT_TARGET="$deployment"
fi
"$@" --release --locked --target "$rust_target"

# The smoke: the artifact exists, runs, and names this release.
if ! exists "$output"; then
    echo "build-target: $missing: $output" >&2
    exit 1
fi
echo "$("$output" --version)" | grep -Fx "upland $version"
