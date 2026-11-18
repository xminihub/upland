#!/usr/bin/env bash
# The C toolchain the two Linux targets build with. Bundled SQLite has to be
# compiled, and a static musl binary linked, for a triple the Ubuntu runners
# have no gcc for; zig cc does both and cargo zigbuild hands it to cargo, which
# is what ADR-0001 names as the answer to a red musl build. Both tools are
# pinned here by version and sha256, so a failure means the pin moved on
# purpose and not that an upstream re-tarred its asset.
#
#   scripts/install-musl-toolchain.sh
#   PREFIX=/opt/upland-tools scripts/install-musl-toolchain.sh
#
# Everything lands under $PREFIX (default "$HOME/.local"): the two commands in
# "$PREFIX/bin", the unpacked archives in "$PREFIX/lib/musl-toolchain". No
# sudo, nothing outside $PREFIX, and a second run with the same pins is a no-op.
set -euo pipefail

zig_version="0.17.1"
zigbuild_version="0.24.1"
prefix="${PREFIX:-$HOME/.local}"
bindir="$prefix/bin"
stagedir="$prefix/lib/musl-toolchain"

if [ "$(uname -s)" != "Linux" ]; then
    echo "install-musl-toolchain: zig cc is the release path for Linux targets; $(uname -s) is not supported" >&2
    exit 2
fi

arch="$(uname -m)"
case "$arch" in
    x86_64)
        zig_sha256="3581863e3cbcfd0fce0efc682ce7e9e93da96668a977791d356a26d2a7e9b17a"
        zigbuild_sha256="32581c8e13f27a807bb244aaaac870522f5b51263c766cf77b1a99771ce9fa5c"
        zigbuild_triple="x86_64-unknown-linux-gnu"
        ;;
    aarch64)
        zig_sha256="a1546d5646eb184437918d6635ca6b762a0e889eac09cef720ee7a1ed3fe7795"
        zigbuild_sha256="70bb4c9cc71a6d16211b9079996e3cd588671053312adeff4d043499ed887601"
        zigbuild_triple="aarch64-unknown-linux-gnu"
        ;;
    *)
        echo "install-musl-toolchain: no pinned zig or cargo-zigbuild for $arch" >&2
        exit 2
        ;;
esac

for tool in curl tar sha256sum; do
    command -v "$tool" >/dev/null || {
        echo "install-musl-toolchain: missing $tool" >&2
        exit 1
    }
done

zig_asset="zig-${arch}-linux-${zig_version}.tar.xz"
zig_url="https://ziglang.org/download/${zig_version}/${zig_asset}"
zigbuild_asset="cargo-zigbuild-${zigbuild_triple}.tar.xz"
zigbuild_url="https://github.com/rust-cross/cargo-zigbuild/releases/download/v${zigbuild_version}/${zigbuild_asset}"
zig_dir="$stagedir/zig-$zig_version"
zigbuild_dir="$stagedir/cargo-zigbuild-$zigbuild_version"

# Actions gives every step its own shell, so the directory the tools land in
# has to be registered for the build step to see it — installed or already
# there, since the no-op below would otherwise skip that.
if [ -n "${GITHUB_PATH:-}" ]; then
    echo "$bindir" >> "$GITHUB_PATH"
fi

if [ -x "$zig_dir/zig" ] && [ -x "$zigbuild_dir/cargo-zigbuild" ] &&
    [ -L "$bindir/zig" ] && [ -L "$bindir/cargo-zigbuild" ]; then
    echo "zig $zig_version and cargo-zigbuild $zigbuild_version are already in $bindir"
    exit 0
fi

install() { # install <url> <sha256> <asset> <staged dir> <binary>
    local url="$1" sha256="$2" asset="$3" dir="$4" binary="$5"
    echo "downloading $asset"
    curl -fsSL --retry 3 -o "$workdir/$asset" -- "$url"
    if ! printf '%s  %s\n' "$sha256" "$asset" | (cd "$workdir" && sha256sum --check --status); then
        echo "install-musl-toolchain: sha256 does not match the pin for $asset" >&2
        exit 1
    fi
    rm -rf "$dir"
    mkdir -p "$dir"
    # Each archive holds one top-level directory named after the asset;
    # --strip-components drops it, so the pinned version names the directory.
    tar -xJf "$workdir/$asset" -C "$dir" --strip-components=1
    rm -f "$workdir/$asset"
    ln -sfn "$dir/$binary" "$bindir/$binary"
}

workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT
mkdir -p "$bindir" "$stagedir"

install "$zig_url" "$zig_sha256" "$zig_asset" "$zig_dir" zig
install "$zigbuild_url" "$zigbuild_sha256" "$zigbuild_asset" "$zigbuild_dir" cargo-zigbuild

echo "installed zig $("$bindir/zig" version), cargo-zigbuild $("$bindir/cargo-zigbuild" --version | cut -d' ' -f2)"
case ":$PATH:" in
    *":$bindir:"*) ;;
    *) echo "note: $bindir is not on PATH; add it before running cargo zigbuild" >&2 ;;
esac
