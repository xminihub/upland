#!/usr/bin/env bash
# Move the version forward and make it public: bump Cargo.toml and Cargo.lock,
# commit with the bot's identity, tag the release, push the commit to main and
# the tag. Every gate runs *before* this — a red release must leave no version
# to explain (ADR-0001) — and what those gates are is verify's business, not
# this script's. The pushed commit is the released state every later job
# checks out by tag.
#
#   scripts/bump-and-tag.sh --bump patch|minor|major
#     -> the new version bare on stdout, the only stdout line: what the
#        release becomes
#
# Needs cargo set-version (cargo-edit, pinned by the release workflow) and a
# full-history checkout of main with something to push to.
set -euo pipefail

bump=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        --bump)
            bump="${2:-}"
            shift 2
            ;;
        *)
            echo "bump-and-tag: unexpected argument: $1" >&2
            exit 2
            ;;
    esac
done

case "$bump" in
    patch|minor|major) ;;
    *)
        echo "usage: bump-and-tag.sh --bump <patch|minor|major>" >&2
        exit 2
        ;;
esac

if ! command -v jq >/dev/null 2>&1; then
    echo "bump-and-tag: jq is required" >&2
    exit 1
fi

if ! cargo set-version --help >/dev/null 2>&1; then
    echo "bump-and-tag: cargo set-version is required (cargo install cargo-edit --version 0.13.13 --locked)" >&2
    exit 1
fi

# Progress goes to stderr throughout: stdout carries exactly the version.
cargo set-version --bump "$bump" 1>&2
version="$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[0].version')"

# The same version shape an asset name must have (see asset-name.sh): a tag
# is v<version>, so a surprise here would mis-shape every name downstream.
if ! printf '%s' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$'; then
    echo "bump-and-tag: cargo metadata did not yield a release version: ${version:-<empty>}" >&2
    exit 1
fi

# The push is attributable to the release, not to whoever dispatched it.
git config user.name "github-actions[bot]"
git config user.email "41898282+github-actions[bot]@users.noreply.github.com"

git add Cargo.toml Cargo.lock
git commit -m "release: v$version" 1>&2
git tag "v$version"
git push origin main 1>&2
git push origin "v$version" 1>&2

printf '%s\n' "$version"
