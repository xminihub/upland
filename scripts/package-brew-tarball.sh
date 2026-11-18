#!/usr/bin/env bash
# Pack one brew target's binary into the tarball the org tap pins.
#
# The archive has exactly one member, named after the binary, with the USTAR
# header fields that would otherwise vary between runs pinned: mode 0755, uid and
# gid 0, mtime at the epoch, and gzip's own name/mtime fields suppressed. So
# republishing a version cannot leave Formula/upland.rb disagreeing with the
# asset it pins (ADR-0001), and `bin.install "upland"` finds the program at the
# archive root rather than under a directory.
#
#   scripts/package-brew-tarball.sh --binary dist/upland-0.1.1-linux-x64 \
#       --version 0.1.1 --target linux-x64 --dir dist/assets
#
# Prints the JSON the release asserts on: {"asset","path","sha256"}.
#
# Needs GNU tar. macOS ships bsdtar, whose header defaults differ in exactly the
# fields pinned here; with gnu-tar installed, run this as TAR=gtar.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
table="$script_dir/targets.json"

binary=""
version=""
target=""
dir="."
while [ "$#" -gt 0 ]; do
  case "$1" in
    --binary)
      binary="${2:-}"
      shift 2
      ;;
    --version)
      version="${2:-}"
      shift 2
      ;;
    --target)
      target="${2:-}"
      shift 2
      ;;
    --dir)
      dir="${2:-}"
      shift 2
      ;;
    *)
      echo "package-brew-tarball: unexpected argument: $1" >&2
      exit 2
      ;;
  esac
done

if [ -z "$binary" ] || [ -z "$version" ] || [ -z "$target" ]; then
  echo "usage: package-brew-tarball.sh --binary <path> --version <semver> --target <brew target> --dir <outdir>" >&2
  exit 2
fi

if [ ! -s "$binary" ]; then
  echo "package-brew-tarball: missing or empty binary: $binary" >&2
  exit 1
fi

gnu_tar() {
  local candidate
  for candidate in "${TAR:-}" gtar tar; do
    [ -n "$candidate" ] || continue
    command -v "$candidate" >/dev/null 2>&1 || continue
    if "$candidate" --version 2>/dev/null | grep -q '^tar (GNU tar)'; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done
  return 1
}

if ! tar_bin="$(gnu_tar)"; then
  echo "package-brew-tarball: GNU tar is required, and $(tar --version 2>&1 | head -1) is not it." >&2
  echo "package-brew-tarball: this packs on Linux (the release and CI run it there);" >&2
  echo "package-brew-tarball: to run it on macOS, brew install gnu-tar and set TAR=gtar." >&2
  exit 1
fi

asset="$("$script_dir/asset-name.sh" --version "$version" --target "$target" --kind tarball)"
member="$(jq -r '.bin' "$table")"

mkdir -p "$dir"
out="$(cd "$dir" && pwd)/$asset"

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
cp "$binary" "$stage/$member"
chmod 0755 "$stage/$member"

(
  cd "$stage"
  "$tar_bin" -cf - \
    --format=ustar \
    --owner=0 --group=0 --numeric-owner \
    --mode=0755 \
    --mtime=@0 \
    --sort=name \
    "$member" | gzip -n -9
) >"$out"

if command -v sha256sum >/dev/null 2>&1; then
  sha256="$(sha256sum "$out" | cut -d' ' -f1)"
else
  sha256="$(shasum -a 256 "$out" | cut -d' ' -f1)"
fi

jq -cn --arg asset "$asset" --arg path "$out" --arg sha256 "$sha256" \
  '{asset: $asset, path: $path, sha256: $sha256}'
