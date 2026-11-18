#!/usr/bin/env bash
# The packer's contract, asserted. This is ci.yml's `packaging` job: it is not a
# cargo test, because every assertion here is about GNU tar's output and a
# developer's tar is bsdtar — the two dialects differ in exactly the header
# fields the tap depends on. Run it on Linux (or with gnu-tar installed and
# TAR=gtar) after changing scripts/package-brew-tarball.sh or scripts/targets.json.
#
# What it proves is what a formula must never be wrong about: one member named
# after the binary, mode 0755, uid and gid 0, mtime at the epoch, the payload
# unchanged, and the same bytes hashing to the same sha256 every time — because
# republishing a version is legal.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
table="$script_dir/targets.json"

failures=0
checks=0

ok() {
    checks=$((checks + 1))
    printf 'ok   %s\n' "$*"
}

fail() {
    failures=$((failures + 1))
    printf 'FAIL %s\n' "$*" >&2
}

check() {
    if [ "$1" = "$2" ]; then
        ok "$3"
    else
        fail "$3: expected [$2], got [$1]"
    fi
}

refuses() {
    if "$@" >/dev/null 2>&1; then
        fail "expected a refusal from: $*"
    else
        ok "refused: $*"
    fi
}

for tool in jq python3; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "$tool is required" >&2
        exit 1
    }
done

tar_probe() {
    local candidate="$1"
    command -v "$candidate" >/dev/null 2>&1 || return 1
    "$candidate" --version 2>/dev/null | grep -q '^tar (GNU tar)'
}

if tar_probe "${TAR:-}" || tar_probe tar; then
    :
else
    echo "GNU tar is required: this asserts the archive GNU tar writes." >&2
    echo "On macOS: brew install gnu-tar, then TAR=gtar bash $0" >&2
    exit 1
fi

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d' ' -f1
    else
        shasum -a 256 "$1" | cut -d' ' -f1
    fi
}

bin_name="$(jq -r '.bin' "$table")"
win32_target="$(jq -r '.targets[] | select(.exe) | .name' "$table")"
probe_version="9.8.7"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

echo "== asset names come from the table =="
while IFS=$'\t' read -r target brew exe; do
    expected="$bin_name-$probe_version-$target"
    [ "$exe" = "true" ] && expected="$expected.exe"
    check "$("$script_dir/asset-name.sh" --version "$probe_version" --target "$target" --kind binary)" \
        "$expected" "binary asset name for $target"

    if [ "$brew" = "true" ]; then
        check "$("$script_dir/asset-name.sh" --version "$probe_version" --target "$target" --kind tarball)" \
            "$expected.tar.gz" "tarball asset name for $target"
    else
        refuses "$script_dir/asset-name.sh" --version "$probe_version" --target "$target" --kind tarball
    fi
done < <(jq -r '.targets[] | [.name, (.brew | tostring), (.exe | tostring)] | @tsv' "$table")

check "$(jq -r '.targets | length' "$table")" "5" "five targets in the table"
check "$(jq -r '[.targets[] | select(.brew)] | length' "$table")" "4" "four of them are brew targets"

for bad_version in "v1.2.3" "1.2" "latest"; do
    refuses "$script_dir/asset-name.sh" --version "$bad_version" --target linux-x64 --kind tarball
done
refuses "$script_dir/asset-name.sh" --version "$probe_version" --target linux-x64-musl --kind tarball

echo "== the packer refuses a tar that is not GNU tar =="
printf '#!/bin/sh\nif [ "$1" = "--version" ]; then echo "bsdtar 3.5.3 - libarchive 3.7.4"; exit 0; fi\nexit 1\n' \
    >"$work/not-gnu-tar"
chmod +x "$work/not-gnu-tar"
stub="$work/$bin_name-$probe_version-linux-x64"
printf 'upland stub payload for the packaging assertions\n' >"$stub"
chmod 0755 "$stub"
refuses env TAR="$work/not-gnu-tar" "$script_dir/package-brew-tarball.sh" \
    --binary "$stub" --version "$probe_version" --target linux-x64 --dir "$work"

echo "== packing the stub twice =="
packed_a="$("$script_dir/package-brew-tarball.sh" --binary "$stub" --version "$probe_version" \
    --target linux-x64 --dir "$work/a")"
packed_b="$("$script_dir/package-brew-tarball.sh" --binary "$stub" --version "$probe_version" \
    --target linux-x64 --dir "$work/b")"

asset="$(jq -r '.asset' <<<"$packed_a")"
path_a="$(jq -r '.path' <<<"$packed_a")"
path_b="$(jq -r '.path' <<<"$packed_b")"
check "$asset" "$bin_name-$probe_version-linux-x64.tar.gz" "the reported asset name"
check "$(basename "$path_a")" "$asset" "the written file is named after the asset"
check "$(sha256_of "$path_a")" "$(jq -r '.sha256' <<<"$packed_a")" \
    "the reported sha256 is sha256sum of the file"
check "$(jq -r '.sha256' <<<"$packed_a")" "$(jq -r '.sha256' <<<"$packed_b")" \
    "two runs report the same sha256"
if cmp -s "$path_a" "$path_b"; then
    ok "packing the same bytes twice produces the same archive"
else
    fail "two runs produced different archives: republishing a version would move its sha256"
fi

echo "== the archive holds one pinned member =="
header_problems="$(python3 - "$path_a" "$stub" "$bin_name" <<'PY'
import sys
import tarfile

archive_path, source_path, member = sys.argv[1:4]
problems = []
extracted = None

with tarfile.open(archive_path) as archive:
    members = archive.getmembers()
    if len(members) != 1:
        problems.append(f"{len(members)} members, expected one: {[m.name for m in members]}")
    else:
        info = members[0]
        for field, expected in (
            ("name", member),
            ("mode", 0o755),
            ("uid", 0),
            ("gid", 0),
            ("mtime", 0),
        ):
            got = getattr(info, field)
            if got != expected:
                problems.append(f"{field} is {got!r}, expected {expected!r}")
        if not info.isfile():
            problems.append("the member is not a regular file")
        if info.uname or info.gname:
            problems.append(f"the member carries an owner name: {info.uname!r}/{info.gname!r}")
        if info.isfile():
            extracted = archive.extractfile(info).read()

if extracted is not None:
    with open(source_path, "rb") as handle:
        if handle.read() != extracted:
            problems.append("the archived bytes differ from the binary that went in")

print("\n".join(problems))
PY
)"
if [ -z "$header_problems" ]; then
    ok "one member named $bin_name: mode 0755, uid/gid 0, mtime at the epoch, payload unchanged"
else
    while IFS= read -r problem; do
        [ -n "$problem" ] && fail "$problem"
    done <<<"$header_problems"
fi

echo "== the packer refuses what it should =="
refuses "$script_dir/package-brew-tarball.sh" --binary "$stub" --version "$probe_version" \
    --target "$win32_target" --dir "$work/reject"
refuses "$script_dir/package-brew-tarball.sh" --binary "$work/empty" --version "$probe_version" \
    --target linux-x64 --dir "$work/reject"
refuses "$script_dir/package-brew-tarball.sh" --binary "$stub" --target linux-x64 --dir "$work/reject"
refuses "$script_dir/package-brew-tarball.sh" --binary "$stub" --version "$probe_version" --dir "$work/reject"
refuses "$script_dir/package-brew-tarball.sh" --binary "$stub" --version v1.2.3 --target linux-x64 --dir "$work/reject"

printf '\n%d checks, %d failures\n' "$checks" "$failures"
[ "$failures" -eq 0 ]
