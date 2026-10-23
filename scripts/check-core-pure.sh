#!/bin/sh
#
# src/core must stay free of GTK, GIO and GLib so its tests need no display.
#
#   scripts/check-core-pure.sh
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

if grep -rnE '(^|[^a-z_])(gtk|gio|glib|adw|sourceview5)(::|!)|use (gtk|gio|glib|adw|sourceview5)\b' src/core; then
    echo "core-pure: src/core must not use gtk, gio, glib, adw or sourceview5" >&2
    exit 1
fi
echo "core-pure: ok"
