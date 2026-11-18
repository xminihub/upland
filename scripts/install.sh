#!/bin/sh
# upland installer — the binary channel's transport (ADR-0006).
#
# POSIX sh, so it runs under the stock shell of any Linux or macOS host;
# Windows downloads the .exe from the release page instead. The release
# workflow attaches this file to every release verbatim as the versionless
# asset `install.sh`, which is what makes a stable download URL possible
# beside version-named assets: the bytes a user executes are pinned to a
# release, and this repository copy is what they can read first.
#
# Deliberately not gated by CI: its failure mode is first-run-loud against
# real assets, and a fake-release fixture would be a second contract to
# maintain. Read it, then:
#
#   curl -fsSL https://github.com/xminihub/upland/releases/latest/download/install.sh | sh
#
# environment:
#   UPLAND_VERSION   the release to install (default: latest; v-prefix optional)
# options:
#   --bin-dir DIR    where the binary lands (default: $HOME/.local/bin)
#   -h, --help       this text

set -eu

REPO="xminihub/upland"
BIN="upland"

say() { printf '%s\n' "$*"; }
fail() { printf 'install.sh: %s\n' "$*" >&2; exit 1; }

usage() {
  say "usage: install.sh [--bin-dir DIR] [-h|--help]"
  say ""
  say "environment:"
  say "  UPLAND_VERSION  release to install (default: latest; v-prefix optional)"
  say ""
  say "installs the $BIN server for this machine's target into \$HOME/.local/bin"
  say "(never elevated; --bin-dir overrides), verifying the download against"
  say "the release's SHA256SUMS before anything moves."
}

bin_dir="${HOME}/.local/bin"
while [ "$#" -gt 0 ]; do
  case "$1" in
    --bin-dir)
      [ "$#" -ge 2 ] || fail "--bin-dir needs a directory"
      bin_dir="$2"
      shift 2
      ;;
    --bin-dir=*)
      bin_dir="${1#--bin-dir=}"
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      fail "unknown argument: $1"
      ;;
  esac
done

command -v curl >/dev/null 2>&1 || fail "curl is required"
command -v tar >/dev/null 2>&1 || fail "tar is required"

# uname onto the target table's names (ADR-0001): a machine this table does
# not describe is refused, not guessed at.
os="$(uname -s)"
cpu="$(uname -m)"
case "$os" in
  Darwin) target_os="darwin" ;;
  Linux) target_os="linux" ;;
  *) fail "unsupported OS: $os — on Windows download the .exe release asset instead" ;;
esac
case "$cpu" in
  arm64|aarch64) target_cpu="arm64" ;;
  x86_64|amd64) target_cpu="x64" ;;
  *) fail "unsupported architecture: $cpu" ;;
esac
target="${target_os}-${target_cpu}"

# Resolve the version. Pinned: UPLAND_VERSION (the pinned one-liner form is
# releases/download/vX.Y.Z/install.sh, whose assets are the same release).
# Unpinned: one redirect hop onto `latest`, never an unauthenticated API call
# with its rate limit.
version="${UPLAND_VERSION:-}"
if [ -z "$version" ]; then
  version="$(curl -fsSLI -o /dev/null -w '%{url_effective}' \
    "https://github.com/${REPO}/releases/latest" | sed -n 's#.*/tag/v##p')"
  [ -n "$version" ] || fail "cannot resolve the latest release of ${REPO}"
else
  version="${version#v}"
fi

asset="${BIN}-${version}-${target}.tar.gz"
base="https://github.com/${REPO}/releases/download/v${version}"

workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT

say "installing upland ${version} for ${target}"
curl -fsSL "${base}/${asset}" -o "${workdir}/${asset}" ||
  fail "v${version} publishes no ${asset} — wrong UPLAND_VERSION, or a release predating the current asset names"
curl -fsSL "${base}/SHA256SUMS" -o "${workdir}/SHA256SUMS" ||
  fail "v${version} has no SHA256SUMS — refusing to install an unverifiable artifact"

if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "${workdir}/${asset}" | cut -d' ' -f1)"
elif command -v shasum >/dev/null 2>&1; then
  actual="$(shasum -a 256 "${workdir}/${asset}" | cut -d' ' -f1)"
else
  fail "need sha256sum or shasum to verify the download"
fi

expected="$(awk -v a="${asset}" '$2 == a { print $1 }' "${workdir}/SHA256SUMS")"
[ -n "$expected" ] || fail "SHA256SUMS names no line for ${asset}"
[ "$actual" = "$expected" ] ||
  fail "sha256 mismatch for ${asset}: ${actual} != ${expected} — refusing to install"

tar xzf "${workdir}/${asset}" -C "$workdir" # the one member is the binary
[ -f "${workdir}/${BIN}" ] || fail "${asset} does not contain ${BIN}"

mkdir -p "$bin_dir"
mv "${workdir}/${BIN}" "${bin_dir}/${BIN}"
chmod 0755 "${bin_dir}/${BIN}"

say "installed ${bin_dir}/${BIN}"
case ":${PATH}:" in
  *":${bin_dir}:"*) ;;
  *) say "note: ${bin_dir} is not on your PATH — add it, or run ${bin_dir}/${BIN} by full path" ;;
esac
say "run it with: ${BIN}   (loopback :8080 by default; --host/--port to choose)"
