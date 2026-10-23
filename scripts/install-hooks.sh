#!/bin/sh
#
# Turns on the repository's git hooks (scripts/git-hooks) for this clone.
#
#   scripts/install-hooks.sh             enable
#   scripts/install-hooks.sh --remove    disable
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

case "${1:-}" in
--remove)
    git config --local --unset core.hooksPath || true
    echo "git hooks disabled"
    ;;
"")
    chmod +x scripts/git-hooks/*
    git config --local core.hooksPath scripts/git-hooks
    echo "git hooks enabled: pre-push runs scripts/ci.sh --quick (bypass with git push --no-verify)"
    ;;
*)
    echo "usage: $0 [--remove]" >&2
    exit 2
    ;;
esac
