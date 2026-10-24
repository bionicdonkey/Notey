#!/bin/sh
#
# The whole automated check, as CI runs it. Exits non-zero on the first failing stage.
#
#   scripts/ci.sh            everything
#   scripts/ci.sh --quick    skip clippy and the release build (what to run while developing)
#
# Window and process tests need a display; without one this script wraps cargo in `xvfb-run -a`.
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

run=""
if [ -z "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]; then
    if ! command -v xvfb-run > /dev/null 2>&1; then
        echo "ci: no display and no xvfb-run; install xvfb or run this from a desktop session" >&2
        exit 1
    fi
    run="xvfb-run -a"
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
