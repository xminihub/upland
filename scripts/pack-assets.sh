#!/usr/bin/env bash
# Turn the per-target build binaries into the release's asset set: one brew
# tarball per brew target, one named executable per exe target, the installer
# byte-for-byte, the checksum manifest file, and the structured notes body the
# GitHub Release carries.
#
# The records come first. The packer and the namer print the asset name, path
# and sha256 they wrote, and those manifest records — not a second reading of
# the directory — are what SHA256SUMS, assert-assets.sh and the notes derive
# from: one derivation per fact (ADR-0001). The packer stays the only tarball
# writer, so determinism has one home too.
#
# The installer is the one release asset the manifest does not name: the
# checksum manifest names the bytes a channel installs, and the script is
# transport, not artifact (ADR-0006) — keeping it out of the records keeps the
# asset-per-target guard exact. The notes file lives outside the assets
# directory, where it can never be uploaded as an asset.
#
#   scripts/pack-assets.sh --version 0.1.1
#     -> dist/manifest.jsonl, dist/assets/*, dist/notes.md (by default)
#
# Packs on Linux, like the packer it drives; sha256sum is assumed.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
table="$script_dir/targets.json"
installer="$script_dir/install.sh"

version=""
binaries_dir="dist/binaries"
assets_dir="dist/assets"
manifest="dist/manifest.jsonl"
notes="dist/notes.md"
# The repository the installer URL points at; in Actions it is given, and the
# fallback names the same home scripts/install.sh hard-codes (ADR-0006).
repo="${GITHUB_REPOSITORY:-xminihub/upland}"
while [ "$#" -gt 0 ]; do
    case "$1" in
        --version)
            version="${2:-}"
            shift 2
            ;;
        --binaries-dir)
            binaries_dir="${2:-}"
            shift 2
            ;;
        --assets-dir)
            assets_dir="${2:-}"
            shift 2
            ;;
        --manifest)
            manifest="${2:-}"
            shift 2
            ;;
        --notes)
            notes="${2:-}"
            shift 2
            ;;
        --repo)
            repo="${2:-}"
            shift 2
            ;;
        *)
            echo "pack-assets: unexpected argument: $1" >&2
            exit 2
            ;;
    esac
done

if [ -z "$version" ]; then
    echo "usage: pack-assets.sh --version <semver> [--binaries-dir <dir>] [--assets-dir <dir>] [--manifest <file>] [--notes <file>] [--repo <owner/repo>]" >&2
    exit 2
fi

# The version's shape is not re-checked here: every name and tarball this
# packer drives goes through asset-name.sh, which owns that rule, and refuses
# a non-release version by name.
mkdir -p "$assets_dir" "$(dirname "$manifest")" "$(dirname "$notes")"
: > "$manifest"

# Brew targets go through the packer, which prints the record it wrote.
for target in $(jq -r '.targets[] | select(.brew) | .name' "$table"); do
    bash "$script_dir/package-brew-tarball.sh" \
        --binary "$binaries_dir/binary-$target/upland" \
        --version "$version" \
        --target "$target" \
        --dir "$assets_dir" | tee -a "$manifest"
done

# Exe targets ship the compiler's bytes under their release name (ADR-0001):
# renamed, then recorded with the same shape of line the packer printed.
for target in $(jq -r '.targets[] | select(.exe) | .name' "$table"); do
    asset="$("$script_dir/asset-name.sh" --version "$version" --target "$target" --kind binary)"
    cp "$binaries_dir/binary-$target/upland.exe" "$assets_dir/$asset"
    jq -cn --arg asset "$asset" \
        --arg path "$PWD/$assets_dir/$asset" \
        --arg sha256 "$(sha256sum "$assets_dir/$asset" | cut -d' ' -f1)" \
        '{asset: $asset, path: $path, sha256: $sha256}' >> "$manifest"
done

# The checksum file names the exact bytes a channel installed, tarballs and
# the Windows executable alike.
jq -sr 'sort_by(.asset)[] | "\(.sha256)  \(.asset)"' "$manifest" > "$assets_dir/SHA256SUMS"
cat "$assets_dir/SHA256SUMS"

cp "$installer" "$assets_dir/install.sh"

# The structured notes body: what this release ships, hash by hash, how to
# install it, and the table it was built from. --generate-notes still appends
# GitHub's own change list below this.
{
    echo "## Install"
    echo
    echo '```sh'
    echo "curl -fsSL https://github.com/$repo/releases/latest/download/install.sh | sh"
    echo '```'
    echo
    echo "The installer picks the target, verifies the checksum against \`SHA256SUMS\`"
    echo "before anything moves, and installs to \`~/.local/bin\`."
    echo
    echo "## Assets"
    echo
    echo "| Asset | SHA-256 |"
    echo "| --- | --- |"
    jq -sr 'sort_by(.asset)[] | "| \(.asset) | `\(.sha256)` |"' "$manifest"
    echo
    echo "\`install.sh\` ships beside these assets as transport, not artifact, so the"
    echo "checksum manifest does not name it."
    echo
    echo "## Targets"
    echo
    echo "| Target | Rust triple | Ships as |"
    echo "| --- | --- | --- |"
    jq -r '.targets[] | "| \(.name) | `\(.rustTarget)` | \(if .brew then "tarball asset" else "the binary itself" end) |"' "$table"
} > "$notes"

echo "pack-assets: $(jq -sr 'length' "$manifest") assets in $assets_dir, records in $manifest, notes in $notes" >&2
