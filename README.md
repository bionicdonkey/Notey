# Notey

**NoteyMcNoteface** (Notey for short) is a native text editor for Linux, written in Rust with GTK 4,
libadwaita and GtkSourceView 5. It is built for GNOME and Zorin OS and looks and behaves like a
native app: it follows the system light and dark theme and uses standard shortcuts.

## Purpose

Notey is a simple, native text editor for the GNOME desktop, in the spirit of Notepad++ and Windows
Notepad: open a file, jot a note, and close the window without a second thought. Like those editors it
remembers your session, so every tab, including untitled and unsaved ones, is back the way you left it
after a restart or a crash. It does this with a private session copy; autosave never writes to your
files, which only change when you save them.

It aims to bring that workflow to GNOME as a native GTK 4 app, with the everyday tools of a code and
text editor around it: syntax highlighting, comment toggling, find and replace with regular
expressions, and proper handling of text encodings and line endings.

## Features

**Tabs and session**
- Tabs for any number of files and untitled notes; pin, reorder, and same-named files show their folder
- Unsaved text is snapshotted a second after you stop typing, again every 30 seconds, and on quit,
  `SIGTERM`, `SIGHUP` and `SIGINT`. Text, cursor, selection, scroll position, pin state, language and
  encoding all come back
- Quitting never prompts. Closing a single unsaved tab asks Save / Discard / Cancel
- Reopen Closed Tab (Ctrl+Shift+T) restores the last 20 closed tabs, unsaved text included
- A file that changed on disk while you were away is flagged with Reload / Keep Mine instead of being
  overwritten. The session store is crash-safe: atomic writes, a backup manifest, and a session
  written by a newer version is never overwritten

**Files**
- New, Open (several at once), Save, Save As, Save All, Reload, Rename, recent files, drag and drop
- `notey FILE`, `notey FILE:LINE` and `notey +LINE FILE`; a second `notey` opens its files in the
  running window, so "Open With" from a file manager adds a tab
- Saves are atomic and keep permissions and symlinks; an unchanged file saves byte-identical
- External changes are detected: clean tabs reload silently, edited tabs ask
- Guards for large files (highlighting off above 5 MB, confirmation above 50 MB, refused above
  200 MB), binary files (opened read-only after a prompt) and read-only files

**Interface**
- A slim, icons-only quick-actions bar under the title bar. Choose which of 25 actions appear, group
  them with separators, drag them into order (main menu ▸ View ▸ Customize Quick Actions), or hide it
- File, Edit, Search and View menus live in the main menu, with shortcuts shown beside each item
- Preferences open as an in-window tab (gear button) with categories for General, Editor,
  Indentation, Appearance, Files, Quick actions, Search & privacy and About

**Editing**
- Undo, redo, cut, copy, paste, select all; cut or copy the current line when nothing is selected
- Toggle line comment (Ctrl+/), explicit Comment and Uncomment, block comment (Ctrl+Shift+A), using
  each language's own comment syntax; indent and unindent; auto-indent; bracket matching; optional
  auto-closing brackets
- Duplicate, delete, move and join lines; sort (ascending, descending, ignoring case, numeric),
  remove duplicate and blank lines, reverse, trim whitespace; UPPER, lower, Title and swap case
- Go to line (`LINE` or `LINE:COLUMN`), insert date and time, word wrap, line numbers, whitespace
  display, right-margin guide, minimap, zoom, document statistics
- Indent width is a preference (default 4, 1 to 16), as is spaces versus tabs. A file whose existing
  indentation is recognised keeps its own style; override it per tab from the status bar

**Languages**
- Syntax highlighting from GtkSourceView 5: Rust, C, C++, Python, JavaScript, TypeScript, Go, Java,
  Shell, SQL, HTML, CSS, XML, JSON, YAML, TOML, INI, Markdown, LaTeX, Dockerfile, Makefile, diff and
  many more. Detected from the file name, then the shebang or an editor modeline; override from the
  status bar

**Find and replace**
- Plain text or regular expression (PCRE2: `\1` groups, lookarounds, `^` and `$` at line ends; `$1`
  is accepted too), match case, whole word
- This tab, the selection, or all open tabs with a clickable result list; live match highlighting and
  a counter; replace one or replace all (a single undo step); search history with Up and Down

**Encodings**
- Detects UTF-8, UTF-16 and UTF-32 (with or without a byte order mark) and legacy code pages
  (Windows-125x, ISO-8859-x, KOI8, Shift_JIS, GBK, Big5, EUC-KR and more); shows the encoding in the
  status bar and flags a guess
- Reopen with another encoding, or Save with another encoding; characters the target cannot hold are
  listed before saving, never dropped silently
- LF, CRLF and CR detected, shown and converted; mixed files are called out on save

## Install

### From a package (Debian, Ubuntu, Zorin OS)

Download `notey_<version>_amd64.deb` from the repository's **Releases** page, then:

```bash
sudo apt install ./notey_<version>_amd64.deb
```

Requires GTK 4.12, libadwaita 1.4 and GtkSourceView 5, which Ubuntu 24.04, Zorin OS 18 and newer ship.
`apt` pulls them in. After installing, Notey appears in the app grid and in "Open With" for text files.

To remove it: `sudo apt remove notey`. Your settings and session in `~/.config/notey` and
`~/.local/share/notey` are left alone.

### From source

```bash
sudo apt install build-essential pkg-config \
    libgtk-4-dev libadwaita-1-dev libgtksourceview-5-dev
cargo build --release

sudo install -Dm755 target/release/notey /usr/local/bin/notey
sudo install -Dm644 resources/com.zorin.Notey.desktop /usr/local/share/applications/com.zorin.Notey.desktop
sudo install -Dm644 resources/icons/hicolor/scalable/apps/com.zorin.Notey.svg \
    /usr/local/share/icons/hicolor/scalable/apps/com.zorin.Notey.svg
sudo install -Dm644 resources/icons/hicolor/symbolic/apps/com.zorin.Notey-symbolic.svg \
    /usr/local/share/icons/hicolor/symbolic/apps/com.zorin.Notey-symbolic.svg
```

Needs Rust 1.89 or newer (install with [rustup](https://rustup.rs)); distribution packages of `cargo` are usually too old. To build your own package instead,
run `packaging/deb/build-deb.sh` and install `dist/notey_<version>_<arch>.deb`.

## Usage

```bash
notey                     # open with your last session
notey notes.txt todo.md   # open files (in the running window if there is one)
notey src/main.rs:120     # jump to a line; also: notey +120 src/main.rs
notey --version
```

| Action | Keys | Action | Keys |
| --- | --- | --- | --- |
| New tab / Open / Save | Ctrl+T / Ctrl+O / Ctrl+S | Find / Replace | Ctrl+F / Ctrl+H |
| Save As / Save All | Ctrl+Shift+S / Ctrl+Alt+S | Next / previous match | F3 / Shift+F3 |
| Close tab / Reopen | Ctrl+W / Ctrl+Shift+T | Go to line | Ctrl+G |
| Toggle comment / block | Ctrl+/ / Ctrl+Shift+A | Duplicate / delete line | Ctrl+Shift+D / Ctrl+Shift+K |
| Next / previous tab | Ctrl+Tab / Ctrl+Shift+Tab | Move line | Alt+↑ / Alt+↓ |
| Word wrap | Alt+Z | Zoom in / out / reset | Ctrl+= / Ctrl+- / Ctrl+0 |

The full list is in the main menu under **Keyboard Shortcuts** (Ctrl+?).

Your settings live in `~/.config/notey/settings.json` and the session in `~/.local/share/notey/session/`.
Session files are private (mode 0600) and can be cleared from Preferences ▸ Search & privacy.

## Building and testing

```bash
cargo run                       # a debug build
scripts/ci.sh                   # everything CI runs: purity, clippy, all tests, release build
scripts/ci.sh --quick           # skip clippy and the release build
xvfb-run -a cargo test          # window and process tests need a display
```

A build run from the checkout shows a generic taskbar icon, because the shell matches the window to an
installed launcher. `scripts/dev-install.sh` installs a user-level launcher and icon for it (and
`--uninstall` removes them); restart the app afterwards.

`NOTEY_SESSION_DIR` and `NOTEY_CONFIG_DIR` point a throwaway instance at its own session and settings;
add `NOTEY_NON_UNIQUE=1` so it does not hand its files to an already running Notey.

The automated suite (unit, property, fixture, window and process tests, run on GitHub Actions) is
described in [`docs/testing.md`](docs/testing.md); what automation cannot click is in
[`docs/acceptance.md`](docs/acceptance.md).

## Project layout

| Path | Holds |
| --- | --- |
| `src/core/` | GTK-free logic with unit tests: encodings, line endings, comment toggling, text operations, search queries, session model, languages |
| `src/application/` | Atomic file writes, file I/O, the session store and its single writer thread |
| `src/ui/` | Window, tabs, find bar, status bar menus, quick-actions bar, preferences |
| `tests/` | Integration, property, window-scenario and process tests, plus fixtures |
| `packaging/`, `resources/` | Debian package script, desktop file, icons, metainfo |
| `docs/` | [`plan.md`](docs/plan.md), [`spec.md`](docs/spec.md), [`testing.md`](docs/testing.md), [`acceptance.md`](docs/acceptance.md) |

## Limitations

- Local files only; remote locations (`sftp://` and so on) are not opened yet
- Encodings are those `encoding_rs` provides plus UTF-16/32; CP437, CP850 and EBCDIC are not supported
- Undo history is not kept across restarts (the restored text is the new baseline)
- Mixed line endings are normalised to the majority style when saved

## License

GPL-3.0-or-later. See [`LICENSE`](LICENSE).
