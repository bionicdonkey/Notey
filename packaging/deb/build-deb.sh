#!/usr/bin/env bash
#
# Builds notey_<version>_<arch>.deb from the current checkout.
#
#   packaging/deb/build-deb.sh [output-dir]      (default: dist/)
#
# Needs cargo, dpkg-deb and the GTK 4 / libadwaita / GtkSourceView 5 development
# packages. Run it on the oldest distribution the package should install on: the
# Depends line is computed from the libraries the binary actually links, so a
# build on Ubuntu 24.04 (Zorin OS 18) installs there and on anything newer.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
out="${1:-$root/dist}"
cd "$root"

version="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
arch="$(dpkg --print-architecture)"
name="notey"
app_id="com.zorin.Notey"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
pkg="$stage/pkg"

cargo build --release --locked
binary="target/release/$name"

install -Dm755 "$binary" "$pkg/usr/bin/$name"
strip --strip-unneeded "$pkg/usr/bin/$name"
install -Dm644 "resources/$app_id.desktop" "$pkg/usr/share/applications/$app_id.desktop"
install -Dm644 "resources/icons/hicolor/scalable/apps/$app_id.svg" \
    "$pkg/usr/share/icons/hicolor/scalable/apps/$app_id.svg"
install -Dm644 "resources/icons/hicolor/symbolic/apps/$app_id-symbolic.svg" \
    "$pkg/usr/share/icons/hicolor/symbolic/apps/$app_id-symbolic.svg"
install -Dm644 "packaging/$app_id.metainfo.xml" "$pkg/usr/share/metainfo/$app_id.metainfo.xml"

install -d "$pkg/usr/share/doc/$name"
copyright="$pkg/usr/share/doc/$name/copyright"
cat > "$copyright" <<COPYRIGHT
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: NoteyMcNoteface

Files: *
Copyright: Josh Benoit
License: GPL-3.0-or-later
 On Debian systems, the full text of the GNU General Public License version 3
 can be found in /usr/share/common-licenses/GPL-3.
COPYRIGHT
chmod 644 "$copyright"

# Link-time dependencies come from the binary itself, so the versions track
# whatever the build host provides rather than a hand-kept list.
shlibs="$(
    cd "$stage"
    mkdir -p debian
    printf 'Source: %s\n\nPackage: %s\nArchitecture: any\n' "$name" "$name" > debian/control
    dpkg-shlibdeps -O -e"$root/$binary" 2>/dev/null | sed -n 's/^shlibs:Depends=//p'
)"
if [[ -z "$shlibs" ]]; then
    echo "dpkg-shlibdeps found no dependencies for $binary" >&2
    exit 1
fi

# The language definitions and style schemes live in libgtksourceview-5-common,
# which the library package already depends on; shared-mime-info names the file types.
depends="$shlibs, libgtksourceview-5-common, shared-mime-info"

install -d "$pkg/DEBIAN"
cat > "$pkg/DEBIAN/control" <<CONTROL
Package: $name
Version: $version
Architecture: $arch
Maintainer: Josh Benoit <jbenoit@alera.casa>
Installed-Size: $(du -sk "$pkg/usr" | cut -f1)
Depends: $depends
Section: editors
Priority: optional
Description: Tabbed text editor that remembers your notes between sessions
 NoteyMcNoteface (Notey) is a native text editor for GNOME and Zorin OS. Tabs,
 including unsaved ones, persist across restarts. It highlights code and markup,
 comments and uncomments lines, finds and replaces with regular expressions, and
 converts text encodings and line endings.
CONTROL

find "$pkg" -type d -exec chmod 755 {} +
mkdir -p "$out"
deb="$out/${name}_${version}_${arch}.deb"
dpkg-deb --root-owner-group --build "$pkg" "$deb" > /dev/null
echo "$deb"
