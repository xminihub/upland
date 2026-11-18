#!/usr/bin/env bash
# The one place a release asset name is spelled out. Every name comes from the
# target table (scripts/targets.json), so the build that produced an artifact
# and the tap that pins it cannot drift apart (ADR-0001).
#
#   scripts/asset-name.sh --version 0.1.1 --target darwin-arm64 --kind tarball
#     -> upland-0.1.1-darwin-arm64.tar.gz
#   scripts/asset-name.sh --version 0.1.1 --target win32-x64 --kind binary
#     -> upland-0.1.1-win32-x64.exe
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
table="$script_dir/targets.json"

version=""
target=""
kind="binary"
while [ "$#" -gt 0 ]; do
    case "$1" in
        --version)
            version="${2:-}"
            shift 2
            ;;
        --target)
            target="${2:-}"
            shift 2
            ;;
        --kind)
            kind="${2:-}"
            shift 2
            ;;
        *)
            echo "asset-name: unexpected argument: $1" >&2
            exit 2
            ;;
    esac
done

if [ -z "$version" ] || [ -z "$target" ]; then
    echo "usage: asset-name.sh --version <semver> --target <target> --kind <binary|tarball>" >&2
    exit 2
fi

if ! command -v jq >/dev/null 2>&1; then
    echo "asset-name: jq is required" >&2
    exit 1
fi

if ! printf '%s' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$'; then
    echo "asset-name: not a release version: $version (a tag is v<version>, an asset is not)" >&2
    exit 1
fi

row="$(jq -c --arg target "$target" '.targets[] | select(.name == $target)' "$table")"
if [ -z "$row" ]; then
    echo "asset-name: not a target: $target (expected $(jq -r '[.targets[].name] | join(", ")' "$table"))" >&2
    exit 1
fi

bin="$(jq -r '.bin' "$table")"
case "$kind" in
    binary)
        suffix="$(jq -r 'if .exe then ".exe" else "" end' <<<"$row")"
        ;;
    tarball)
        if ! jq -e '.brew' <<<"$row" >/dev/null; then
            echo "asset-name: not a brew target: $target ($(jq -r '[.targets[] | select(.brew) | .name] | join(", ")' "$table") are)" >&2
            exit 1
        fi
        suffix=".tar.gz"
        ;;
    *)
        echo "asset-name: unknown kind: $kind (expected binary or tarball)" >&2
        exit 2
        ;;
esac

printf '%s-%s-%s%s\n' "$bin" "$version" "$target" "$suffix"
