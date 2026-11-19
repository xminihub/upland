#!/usr/bin/env bash
# The Gate job's whole content: render the run summary and colour the gate.
# The workflow hands one argument per lane, "Name=conclusion"; every lane is
# listed with its conclusion, and the script exits green only when all of them
# succeeded — the exit code IS the gate's colour, one derivation for the run's
# single green statement. The markdown lands in $GITHUB_STEP_SUMMARY, the file
# the runner publishes on the run page; its contract is asserted by
# scripts/test-ci-gate-summary.sh, not only by a live run.
set -euo pipefail

summary="${GITHUB_STEP_SUMMARY:?set by the runner: the file the summary markdown lands in}"

# Refuse rather than render: a vacuous or malformed lane list would otherwise
# write a summary that reads as checked when nothing was. Exit 2 for a wiring
# mistake keeps it distinct from exit 1, a lane that is genuinely not green.
[ "$#" -gt 0 ] || {
    echo "usage: ci-gate-summary.sh Lane=conclusion [Lane=conclusion ...]" >&2
    exit 2
}
for arg in "$@"; do
    [[ "$arg" =~ ^[A-Za-z0-9._-]+=[A-Za-z0-9._-]+$ ]] || {
        echo "malformed lane=conclusion argument: [$arg]" >&2
        exit 2
    }
done

rc=0
{
    printf '## Gate\n\n'
    for arg in "$@"; do
        lane="${arg%%=*}"
        conclusion="${arg#*=}"
        case "$conclusion" in
            success)   mark='✅' ;;
            skipped)   mark='⏭️' ;;
            cancelled) mark='🚫' ;;
            *)         mark='❌' ;;
        esac
        [ "$conclusion" = "success" ] || rc=1
        printf -- '- %s **%s** — %s\n' "$mark" "$lane" "$conclusion"
    done
    # The identity line exists exactly when the runner can source it: a local
    # invocation gets the lanes without an invented run link.
    if [ -n "${GITHUB_RUN_ID:-}" ]; then
        sha7="$(printf '%s' "${GITHUB_SHA:-}" | cut -c1-7)"
        printf '\n_%s on `%s` by %s · [run #%s](%s/%s/actions/runs/%s)_\n' \
            "$sha7" "${GITHUB_REF_NAME:-}" "${GITHUB_ACTOR:-}" \
            "$GITHUB_RUN_ID" "${GITHUB_SERVER_URL:-https://github.com}" \
            "${GITHUB_REPOSITORY:-}" "$GITHUB_RUN_ID"
    fi
} >"$summary"

exit "$rc"
