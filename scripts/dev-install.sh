#!/bin/sh
#
# Gives a build run from the source tree its icon and an "Open With" entry, without a package.
# The taskbar and dock match a window's app id (com.zorin.Notey) to an installed .desktop file, so
# an uninstalled debug build otherwise shows a generic icon.
#
#   scripts/dev-install.sh                 launcher for target/debug/notey
#   scripts/dev-install.sh --release       launcher for target/release/notey
#   scripts/dev-install.sh --uninstall     remove what this script installed
#
# Everything goes under $XDG_DATA_HOME (default ~/.local/share); nothing needs root. Start the app
# again afterwards (the shell reads the launcher when a window first appears).
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
id="com.zorin.Notey"
apps="$data/applications"
scalable="$data/icons/hicolor/scalable/apps"
symbolic="$data/icons/hicolor/symbolic/apps"

refresh() {
    command -v update-desktop-database > /dev/null 2>&1 && update-desktop-database "$apps" 2> /dev/null || true
    command -v gtk4-update-icon-cache > /dev/null 2>&1 && gtk4-update-icon-cache -q -t -f "$data/icons/hicolor" 2> /dev/null || true
    command -v gtk-update-icon-cache > /dev/null 2>&1 && gtk-update-icon-cache -q -t -f "$data/icons/hicolor" 2> /dev/null || true
}

case "${1:-}" in
--uninstall)
    rm -f "$apps/$id.desktop" "$scalable/$id.svg" "$symbolic/$id-symbolic.svg"
    refresh
    echo "removed the development launcher and icons from $data"
    exit 0
    ;;
--release) profile=release ;;
"") profile=debug ;;
*)
    echo "usage: $0 [--release | --uninstall]" >&2
    exit 2
    ;;
esac

binary="$root/target/$profile/notey"
if [ ! -x "$binary" ]; then
    echo "dev-install: $binary does not exist yet; run 'cargo build${profile:+ }$([ "$profile" = release ] && echo --release)' first" >&2
    exit 1
fi

mkdir -p "$apps" "$scalable" "$symbolic"
cp "$root/resources/icons/hicolor/scalable/apps/$id.svg" "$scalable/$id.svg"
cp "$root/resources/icons/hicolor/symbolic/apps/$id-symbolic.svg" "$symbolic/$id-symbolic.svg"
# The packaged launcher with Exec pointing at this checkout's binary.
sed "s|^Exec=.*|Exec=$binary %F|" "$root/resources/$id.desktop" > "$apps/$id.desktop"
chmod 644 "$apps/$id.desktop"
refresh
echo "installed a launcher for $binary"
echo "  $apps/$id.desktop"
echo "Quit and start Notey again; the taskbar should now show the icon."
echo "Undo with: scripts/dev-install.sh --uninstall"
