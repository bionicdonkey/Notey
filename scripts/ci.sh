#!/bin/sh
#
# The whole automated check, as CI runs it. Exits non-zero on the first failing stage.
#
#   scripts/ci.sh            everything
#   scripts/ci.sh --quick    skip clippy and the release build (what to run while developing)
#
# Window and process tests open real windows, so this script always runs them under `xvfb-run -a`,
# even from a desktop session, and never on your own screen. NOTEY_ALLOW_REAL_DISPLAY=1 opts out.
# NOTEY_REQUIRE_DISPLAY makes a missing display a failure rather than a silent skip, and
# NOTEY_STRICT_WARNINGS fails a window scenario on any GLib/GTK warning or critical.
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

quick=0
[ "${1:-}" = "--quick" ] && quick=1

stage() { printf '\n== %s\n' "$1"; }

export RUST_BACKTRACE=1
export NOTEY_REQUIRE_DISPLAY=1
export NOTEY_STRICT_WARNINGS=1

stage "core stays free of GTK"
sh scripts/check-core-pure.sh

if [ "$quick" -eq 0 ]; then
    stage "clippy (warnings are errors)"
    cargo clippy --locked --all-targets -- -D warnings
fi

# The tests refuse a display unless told it is not the user's own session.
export NOTEY_ISOLATED_DISPLAY=1
run=""
if [ -n "${NOTEY_ALLOW_REAL_DISPLAY:-}" ] && [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]; then
    :
elif command -v xvfb-run > /dev/null 2>&1; then
    run="xvfb-run -a"
else
    echo "ci: no xvfb-run; install xvfb (or set NOTEY_ALLOW_REAL_DISPLAY=1 to open windows on your own session)" >&2
    exit 1
fi

# Where the test output is kept (CI uploads it). The Docker wrapper points this at a writable volume.
log="${NOTEY_CI_LOG:-target/ci-test.log}"
mkdir -p "$(dirname "$log")"
stage "tests: unit, properties, fixtures, window scenarios, process smoke"
# shellcheck disable=SC2086
if ! $run cargo test --locked --no-fail-fast 2>&1 | tee "$log"; then
    echo "ci: tests failed; full output in $log" >&2
    exit 1
fi
# `tee` hides cargo's status in a plain pipe on some shells, so check the log as well.
if grep -qE '^test result: FAILED|^window scenarios: [0-9]+ failed|^error: ' "$log"; then
    echo "ci: tests failed; full output in $log" >&2
    exit 1
fi

if [ "$quick" -eq 0 ]; then
    stage "release build and version check"
    cargo build --release --locked
    ./target/release/notey --version
fi

stage "summary"
grep -E '^test result:|^window scenarios:' "$log"
echo "ci: all stages passed"
