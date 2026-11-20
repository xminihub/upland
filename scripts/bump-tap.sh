#!/usr/bin/env bash
# The tap owns Formula/upland.rb: this dispatches its bump workflow and waits
# for the run that bumps *this tool to this version*, then checks the formula
# itself. A release whose formula is not pinned is a red release rather than a
# discovery for a user (ADR-0001). The token is Actions-only on the tap, so
# nothing here holds content write access to it.
#
# A dispatch is only recognisable as ours from its job name,
# "Bump <tool> to <version>": the run itself is titled after the workflow, so
# another tool's bump started in the same window would otherwise be watched
# and reported as ours.
#
# The run's conclusion is not the contract; a version pinned in the formula
# is. Reading the formula back is what makes "a green release means an
# installable formula" true even if the watched run turned out not to be
# ours, or a repair re-dispatch already landed the same version — a failed
# run that pinned the right version is a warning, not a red release, because
# the channel is what the gate is about.
#
#   scripts/bump-tap.sh --repo <owner/repo> --tool <name> --version <semver>
#
# Needs gh with GH_TOKEN set; the caller's job timeout is what bounds the
# watch.
set -euo pipefail

repo=""
tool=""
version=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        --repo)
            repo="${2:-}"
            shift 2
            ;;
        --tool)
            tool="${2:-}"
            shift 2
            ;;
        --version)
            version="${2:-}"
            shift 2
            ;;
        *)
            echo "bump-tap: unexpected argument: $1" >&2
            exit 2
            ;;
    esac
done

if [ -z "$repo" ] || [ -z "$tool" ] || [ -z "$version" ]; then
    echo "usage: bump-tap.sh --repo <owner/repo> --tool <name> --version <semver>" >&2
    exit 2
fi

started_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
gh workflow run bump.yml --repo "$repo" --ref main \
    -f name="$tool" -f version="$version"

run_id=""
for _ in $(seq 1 60); do
    for candidate in $(gh run list --repo "$repo" --workflow bump.yml \
        --event workflow_dispatch --limit 10 \
        --json databaseId,createdAt \
        --jq "[.[] | select(.createdAt >= \"$started_at\")] | .[].databaseId"); do
        if gh api "repos/$repo/actions/runs/$candidate/jobs" \
            --jq '.jobs[].name' | grep -Fxq "Bump $tool to $version"; then
            run_id="$candidate"
            break
        fi
    done
    [ -n "$run_id" ] && break
    sleep 5
done

if [ -z "$run_id" ]; then
    echo "::error::no bump.yml run for $tool $version started after $started_at" >&2
    exit 1
fi
echo "bump run: https://github.com/$repo/actions/runs/$run_id"

if gh run watch "$run_id" --repo "$repo" --exit-status --interval 20; then
    concluded=success
else
    concluded=failed
fi

pinned="$(gh api "repos/$repo/contents/Formula/$tool.rb" --jq .content \
    | base64 --decode | sed -n 's#.*/releases/download/v\([^/]*\)/.*#\1#p' | head -1)"

if [ "$pinned" = "$version" ]; then
    echo "Formula/$tool.rb pins $version (bump run $concluded)"
    [ "$concluded" = success ] ||
        echo "::warning::the bump run $concluded, but the formula already pins $version"
else
    echo "::error::Formula/$tool.rb pins ${pinned:-nothing}, not $version (bump run $concluded: https://github.com/$repo/actions/runs/$run_id)" >&2
    exit 1
fi
