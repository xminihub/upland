#!/usr/bin/env bash
# The gate summary's contract, asserted. ci.yml's Packaging job runs this
# beside test-package-brew-tarball.sh; it is deliberately not a cargo test:
# the behaviour under test is the markdown the
# Gate job writes to its step summary and the exit code that colours the gate.
#
# What it proves is what the Gate job relies on: every lane is named with its
# conclusion, the gate is green iff every lane succeeded, malformed input is
# refused rather than rendered, and the run identity line appears exactly when
# the runner's environment is present to source it.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
gatesh="$script_dir/ci-gate-summary.sh"

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

refuses() {
    if "$@" >/dev/null 2>&1; then
        fail "expected a refusal from: $*"
    else
        ok "refused: $*"
    fi
}

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# The summary file the script is contractually required to write.
export GITHUB_STEP_SUMMARY="$work/summary.md"
# Outside a workflow run there is no runner identity unless a slice asks for it.
unset GITHUB_SHA GITHUB_REF_NAME GITHUB_ACTOR GITHUB_RUN_ID GITHUB_REPOSITORY GITHUB_SERVER_URL

echo "== every lane green: the summary names them all and the gate passes =="
# The runner's workflow env is the identity source; a plain empty-string
# unset would still leave the variables "set" to some shells' ${VAR:-} logic,
# so slices that care re-export them explicitly.
if bash "$gatesh" Web=success Rust=success Packaging=success; then
    ok "all-success input exits green"
else
    fail "all-success input must exit 0, the gate would be red on a green run"
fi
if [ -s "$GITHUB_STEP_SUMMARY" ]; then
    for lane in Web Rust Packaging; do
        if grep -qF "**$lane** — success" "$GITHUB_STEP_SUMMARY"; then
            ok "the summary names $lane with its conclusion"
        else
            fail "the summary does not name $lane as success:
$(cat "$GITHUB_STEP_SUMMARY")"
        fi
    done
else
    fail "the summary file is missing or empty"
fi

echo "== a failed lane: the gate goes red but the summary still names it =="
rm -f "$GITHUB_STEP_SUMMARY"
if bash "$gatesh" Web=success Rust=failure Packaging=success; then
    fail "a failed lane must colour the gate red"
else
    ok "a failed lane colours the gate red"
fi
rust_line="$(grep -F '**Rust**' "$GITHUB_STEP_SUMMARY" || true)"
if [ -n "$rust_line" ] && printf '%s' "$rust_line" | grep -qF '— failure' \
    && ! printf '%s' "$rust_line" | grep -qF '✅'; then
    ok "the summary marks the failed lane redly: $rust_line"
else
    fail "the failed lane is not visibly red in the summary: [$rust_line]"
fi

echo "== skipped and cancelled lanes report themselves, not failure =="
for conclusion in skipped cancelled; do
    rm -f "$GITHUB_STEP_SUMMARY"
    if bash "$gatesh" Web=success "Rust=$conclusion" Packaging=success; then
        fail "a $conclusion lane must not leave the gate green"
    else
        ok "a $conclusion lane colours the gate red"
    fi
    lane_line="$(grep -F '**Rust**' "$GITHUB_STEP_SUMMARY" || true)"
    if printf '%s' "$lane_line" | grep -qF "— $conclusion" \
        && ! printf '%s' "$lane_line" | grep -qE '✅|❌'; then
        ok "the summary marks the $conclusion lane as itself: $lane_line"
    else
        fail "the $conclusion lane is not reported as such: [$lane_line]"
    fi
done

echo "== malformed input is refused, not rendered =="
# Zero lanes would otherwise render an empty "all green" summary: a Gate that
# passes because nothing was checked. Every refusal here also asserts no
# summary file is left claiming success.
rm -f "$GITHUB_STEP_SUMMARY"
refuses bash "$gatesh"
refuses bash "$gatesh" Web
refuses bash "$gatesh" "=success"
refuses bash "$gatesh" "Web="
refuses bash "$gatesh" " =success"
refuses bash "$gatesh" Web=success "Rust=failure" noequals
[ ! -e "$GITHUB_STEP_SUMMARY" ] \
    && ok "no summary file survives a refusal" \
    || fail "a refusal still wrote:
$(cat "$GITHUB_STEP_SUMMARY")"

echo "== no summary file to write into: refused =="
rm -f "$GITHUB_STEP_SUMMARY"
if GITHUB_STEP_SUMMARY= bash "$gatesh" Web=success 2>/dev/null; then
    fail "the script rendered despite an unusable GITHUB_STEP_SUMMARY"
else
    ok "the script refuses an unusable GITHUB_STEP_SUMMARY"
fi

echo "== run identity: present when the runner's env can source it =="
rm -f "$GITHUB_STEP_SUMMARY"
if GITHUB_SHA=0123456789abcdef0123456789abcdef01234567 \
    GITHUB_REF_NAME=main \
    GITHUB_ACTOR=someone \
    GITHUB_REPOSITORY=xminihub/upland \
    GITHUB_RUN_ID=4242 \
    GITHUB_SERVER_URL=https://github.com \
    bash "$gatesh" Web=success Rust=success Packaging=success; then
    summary_text="$(cat "$GITHUB_STEP_SUMMARY")"
    for fact in '0123456' 'main' 'someone' \
        'https://github.com/xminihub/upland/actions/runs/4242'; do
        if printf '%s' "$summary_text" | grep -qF "$fact"; then
            ok "the identity line carries $fact"
        else
            fail "the identity line is missing $fact:
$summary_text"
        fi
    done
else
    fail "runner env present must not colour the gate red"
fi

echo "== run identity: absent outside a runner =="
rm -f "$GITHUB_STEP_SUMMARY"
bash "$gatesh" Web=success Rust=success Packaging=success
identity_free="$(cat "$GITHUB_STEP_SUMMARY")"
if printf '%s' "$identity_free" | grep -qF 'actions/runs'; then
    fail "a local run invented a workflow-run link:
$identity_free"
else
    ok "no identity line without the runner's env"
fi

printf '\n%d checks, %d failures\n' "$checks" "$failures"
[ "$failures" -eq 0 ]
