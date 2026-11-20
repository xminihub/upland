#!/usr/bin/env bash
# The release's asset guard: every target in the table has an asset, the
# manifest names it, and the hashes still hold against the bytes on disk.
# It reads pack-assets.sh's records — not a second inventory of the
# directory — because one derivation per fact is what keeps the count exact
# and the installer honestly excluded (ADR-0001, ADR-0006). A missing or
# moved asset is a red package job, not a release with a 404 waiting.
#
#   scripts/assert-assets.sh
#     -> one "ok <asset>" line per record; exit 1 on the first mismatch
#
# Reads the same defaults pack-assets.sh writes; run it beside that script.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
table="$script_dir/targets.json"

manifest="dist/manifest.jsonl"
assets_dir="dist/assets"
while [ "$#" -gt 0 ]; do
    case "$1" in
        --manifest)
            manifest="${2:-}"
            shift 2
            ;;
        --assets-dir)
            assets_dir="${2:-}"
            shift 2
            ;;
        *)
            echo "assert-assets: unexpected argument: $1" >&2
            exit 2
            ;;
    esac
done

if ! command -v jq >/dev/null 2>&1; then
    echo "assert-assets: jq is required" >&2
    exit 1
fi

if [ ! -s "$manifest" ]; then
    echo "assert-assets: missing or empty manifest: $manifest" >&2
    exit 1
fi

expected="$(jq -r '.targets | length' "$table")"
actual="$(jq -sr 'length' "$manifest")"
if [ "$expected" != "$actual" ]; then
    echo "assert-assets: the manifest holds $actual assets, the target table has $expected targets" >&2
    exit 1
fi

while IFS=$'\t' read -r asset path sha256; do
    if [ ! -s "$path" ]; then
        echo "assert-assets: missing or empty: $asset" >&2
        exit 1
    fi
    actual="$(sha256sum "$path" | cut -d' ' -f1)"
    if [ "$actual" != "$sha256" ]; then
        echo "assert-assets: sha256 moved for $asset: $actual != $sha256" >&2
        exit 1
    fi
    echo "ok $asset"
done < <(jq -sr '.[] | [.asset, .path, .sha256] | @tsv' "$manifest")

ls -la "$assets_dir"
