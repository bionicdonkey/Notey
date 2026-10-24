# Notey: implementation specification

Normative behaviour for v1. "Must" is a requirement with a test or manual acceptance check;
"should" is a default that may be revisited. Ordering of sections is referenced by `docs/plan.md`.

## 1. Platform and dependencies

- Rust 2021, stable toolchain (1.97 here). System: GTK 4.14, libadwaita 1.5, GtkSourceView 5.12
  (`libgtksourceview-5-dev`).
- Crates: `gtk4` 0.8 (`v4_12`), `libadwaita` 0.6 (`v1_5`), `sourceview5` 0.8, `gio`/`glib` 0.19,
  `serde` + `serde_json`, `encoding_rs` 0.8, `chardetng` 1, `uuid` (v4) or an equivalent
  random-id helper. Dev: `tempfile`. Pin transitive crates only if the toolchain demands it.
- The `core` module must not import `gtk`, `gio`, `glib` or `sourceview5`
  (enforced by a `scripts/check-core-pure.sh` grep, as Explorer does with `check-separation.sh`).

## 2. Window and chrome

2.1 One `adw::ApplicationWindow` with `adw::ToolbarView`: header bar on top, `adw::TabBar` beneath
it (autohide off, scrollable), `adw::TabView` as content, status bar at the bottom.
2.2 Header bar: **New tab** (+), **Open**, **Save** (with dropdown: Save As, Save All), search
toggle, primary menu (Preferences, Keyboard Shortcuts, About). Window title = active tab title,
subtitle = directory of the file (or "Unsaved" for untitled).
2.3 Window size, maximised state, minimap, word wrap, zoom, and sidebar-free layout are restored
from settings.
2.4 Follows system dark/light; Preferences offers System / Light / Dark and a GtkSource style
scheme (default `Adwaita` / `Adwaita-dark` pair chosen by theme).

## 3. File operations

3.1 **New** (Ctrl+T, Ctrl+N): untitled tab named `Untitled`, `Untitled 2`, ... (lowest unused number
among open untitled tabs).
3.2 **Open** (Ctrl+O): native file chooser, multi-select. A file already open in a tab activates
that tab instead of opening a second copy (compared by canonical path). Opening replaces an
untouched, empty, untitled active tab (the common "open into fresh window" behaviour).
3.3 **Save** (Ctrl+S): untitled → Save As. Otherwise write with the tab's encoding and line ending
(§9), atomically: write `.<name>.notey-tmp` in the same directory, fsync, rename over the target,
preserve permissions. If the directory is not writable for temp files, fall back to GIO's
replace-in-place and say so in the log. A failed save keeps the tab dirty and shows an
`adw::Toast`/dialog with the OS error; nothing is lost.
3.4 **Save As** (Ctrl+Shift+S): chooser prefilled with the current name. (GTK 4.14's `FileDialog` has no extra-choice area, so
the encoding and line ending are changed beforehand from the status bar; they are kept across Save As.) Overwriting an
existing file is confirmed by the chooser; saving onto a file open in another tab is refused with a
message.
3.5 **Save All** (Ctrl+Alt+S): saves every dirty file-backed tab; untitled dirty tabs are prompted
one by one; cancelling stops the sequence.
3.6 **Size tiers** (checked before reading, using file size):

| Size | Behaviour |
| --- | --- |
| ≤ 5 MB | Normal |
| > 5 MB | Highlighting, minimap and current-line highlight off for that tab; status bar shows "Large file" |
| > 50 MB | Confirm dialog ("Open anyway / Cancel"); then as above, plus search match counting off |
| > 200 MB | Refused with an explanation |

Thresholds are constants in one place and overridable in settings.
3.7 **Binary guard**: if the first 8 KiB contains a NUL byte and the content is not valid
UTF-16/UTF-32 with BOM, ask "This looks like a binary file. Open anyway / Cancel". Opening anyway
decodes with replacement characters and marks the tab read-only.
3.8 **Read-only files** (no write permission) open normally with a "Read-only" indicator; Save
triggers Save As.
3.9 **Reload**: File ▸ Reload from Disk (Ctrl+R), confirms if the tab is dirty.
3.10 **External changes**: a `GFileMonitor` per file-backed tab. On change/delete/rename:
 - If the tab is clean: reload silently and keep cursor line (toast "Reloaded <name>").
 - If dirty: show an in-tab `adw::Banner` "File changed on disk" with **Reload** / **Keep mine**
   (and for deletion "File was deleted" with **Save As…** / **Close**). Never auto-overwrite.
 - Our own saves are suppressed by comparing the post-save mtime/size to the one we recorded.
3.11 **Recent files** (File menu, 15 entries): updated on open and save, de-duplicated, entries
whose file no longer exists are shown disabled and removed on click. Also registered with GTK's
`RecentManager` so the desktop sees them. "Clear recent" present.
3.12 **Command line / desktop**: `notey [FILE...]`, `notey +LINE FILE` and `notey FILE:LINE` jump to
a line. `HANDLES_OPEN` + `MimeType=text/plain;text/markdown;...` in the desktop file. A second
invocation opens tabs in the existing window and presents it.
3.13 **Drag and drop**: dropping files (text/uri-list) onto the window opens them; dropping a tab
on another window is not supported in v1.
3.14 **Encoding/line-ending preservation**: opening and saving an unmodified file must produce
byte-identical output (including BOM, trailing newline presence, and original line endings).
A test round-trips fixtures for each supported encoding and ending.
3.15 **Trailing newline**: the buffer text is stored as-is; Notey does not add or remove a final
newline unless the user sets "Ensure newline at end of file" (default off).

## 4. Tabs

4.1 `adw::TabView`. Tab title = file name, or `Untitled N`. A dirty tab shows a leading `•`
(via the page's "needs attention"/indicator icon plus title style); tooltip shows the full path.
4.2 Close tab: middle-click, the close button, Ctrl+W. A **dirty** tab prompts: **Save** / **Discard** /
**Cancel**. A clean tab closes immediately. Closing the last tab leaves one fresh `Untitled` tab
(the window is closed with Ctrl+Q / the window close button, not by closing the last tab).
4.3 **Reopen closed tab** (Ctrl+Shift+T): keeps the last 20 closed tabs *including their unsaved
content*, in memory for the process lifetime and persisted in the session (§5.4).
4.4 Reorder by drag; Ctrl+PageUp/PageDown and Ctrl+Tab/Ctrl+Shift+Tab cycle; Alt+1…9 jump; tab
context menu: Save, Save As, Close, Close Others, Close to the Right, Pin/Unpin, Copy Path, Copy Name, Show in
Files, Reload from Disk, Rename (file-backed: renames on disk).
4.5 Pinned tabs sit at the left, are compact, and are skipped by Close Others/Close All.
4.6 Duplicate-title disambiguation: two tabs named `main.rs` show `main.rs — src/ui` style
parent-directory suffix.
4.7 Quitting (Ctrl+Q, window close): **no save prompt**; flush the session (§5) synchronously and
exit. This is what makes "tabs persist across restart" true for dirty tabs.

## 5. Session persistence (the core promise)

5.1 **Location**: `$XDG_DATA_HOME/notey/session/` (default `~/.local/share/notey/session/`).
```
session/
  session.json        manifest (versioned)
  session.json.bak    previous manifest
  buffers/<id>.txt    UTF-8 snapshot of an unsaved buffer, only for tabs that need one
```
5.2 **Manifest** (`"version": 1`):
```json
{
  "version": 1,
  "saved_at": 1761300000,
  "active_tab": "6f1c…",
  "tabs": [
    {
      "id": "6f1c…",
      "path": "/home/josh/notes/todo.md",
      "title": "todo.md",
      "pinned": false,
      "dirty": true,
      "buffer_file": "6f1c….txt",
      "base": { "mtime_ns": 1761300000000000000, "size": 1234, "sha256": "…" },
      "encoding": "UTF-8", "bom": false, "line_ending": "lf",
      "language": null,
      "cursor": { "line": 41, "column": 7 },
      "scroll_line": 38,
      "selection": null
    }
  ],
  "closed_tabs": [ /* same shape, max 20 */ ]
}
```
`language: null` = auto. `path: null` = untitled (always has `buffer_file`).
5.3 **What gets a buffer file**: untitled tabs with any content (empty untitled tabs are recorded
without a buffer), and dirty file-backed tabs. Clean file-backed tabs store metadata only; they are
re-read from disk on restore. Buffer files are stored already normalised to `\n`, UTF-8, no BOM;
the original encoding/ending live in the manifest.
5.4 **When snapshots are written**:
 - Debounced **1 s after the last edit** (per tab, only that tab's buffer file is rewritten).
 - Manifest rewritten on: tab open/close/reorder/pin/switch, save, encoding/ending/language change,
   and cursor movement (coalesced, at most every 5 s).
 - A **periodic safety flush every 30 s** if anything is dirty.
 - **Synchronously on quit** and on SIGTERM/SIGHUP (via `glib::unix_signal_add`).
 Writes happen off the UI thread except the quit flush. All writes are atomic: temp file in the same
 directory, fsync, rename. Before replacing `session.json`, copy it to `session.json.bak`.
5.5 **Restore** (on start, before showing the window content):
 1. Read `session.json`; on parse failure fall back to `session.json.bak`; on both failing, start
    with one untitled tab and move the unreadable files to `session/corrupt-<timestamp>/`.
 2. For every tab record, in order: untitled → load buffer file; file-backed + `buffer_file` →
    load buffer file, then compare the file on disk with `base` (§5.7); file-backed clean → read
    from disk.
 3. Restore pin state, active tab, cursor, scroll, selection, language override, encoding/ending.
 4. Delete buffer files not referenced by the manifest (orphans) after a successful restore.
5.6 **Tolerance**: a missing or unreadable buffer file for one tab produces a notice and an
empty (or, if the path exists, re-read) tab; it never prevents other tabs from restoring. A
file-backed clean tab whose file is gone is restored as an empty untitled-style tab named for the
file with a banner "File no longer exists" so nothing is dropped silently. Unknown `version`
greater than ours: read-only best effort, do not overwrite (keeps newer app data safe).
5.7 **Disk changed since snapshot** (dirty file-backed tab whose file's mtime/size/hash differs from
`base`): restore the user's unsaved buffer *and* show the §3.10 banner (Reload / Keep mine) so
neither version is lost; "Keep mine" marks the buffer dirty against the new disk state.
5.8 **Undo history** is not persisted (documented); after restore the undo stack starts empty and
the restored text is the baseline.
5.9 **Multiple windows**: v1 is a single window. `New Window` is not offered.
5.10 **Privacy**: the session directory is created `0700`, buffer files `0600`. A preference
"Remember tabs between sessions" (default on) off clears the session on quit; "Clear saved session"
button in Preferences.
5.11 **Test hooks**: `NOTEY_SESSION_DIR` overrides the location (used by tests and for running a
throwaway instance).

## 6. Editing

6.1 Standard actions with standard shortcuts: Undo (Ctrl+Z), Redo (Ctrl+Shift+Z / Ctrl+Y), Cut, Copy,
Paste, Select All, Delete. Cut/Copy with no selection operate on the current line (Notepad++
convention) when the preference "Cut/copy line when nothing selected" is on (default on).
6.2 **Paste** converts clipboard line endings to `\n`; Ctrl+Shift+V pastes as plain text with
leading/trailing whitespace trimmed (should).
6.3 **View**: line numbers (default on), current-line highlight (on), show whitespace (off; Edit ▸
View toggle; renders spaces/tabs/newlines with GtkSource space drawer), word wrap (off; Alt+Z),
right margin guide (off, 80/100/120 selectable), minimap (`sourceview5::Map`, off), zoom (Ctrl +,
Ctrl −, Ctrl 0, Ctrl+scroll; 50%–400%, per window, persisted).
6.4 **Indentation**: the *indent width* is a preference (`indent_width`, default **4**, range 1–16) and so is *insert spaces vs. tabs* (default spaces). Tab inserts one indent unit (a tab character or `indent_width` spaces); the same width is used for tab-stop display, Shift+Tab, block indent and auto-indent. Changing it applies live to all open tabs that have no per-tab override; Shift+Tab
unindents; Tab with a multi-line selection indents all selected lines. Auto-indent on Enter copies
the previous line's indentation (GtkSource `auto-indent`). Indent style per tab is auto-detected
from the first 200 indented lines (tabs vs. 2/4/8 spaces) and overridable from the status bar.
6.5 **Bracket matching**: highlight matching `()[]{}` (GtkSource). Auto-closing brackets/quotes is a
preference, default off.
6.6 **Go to line** (Ctrl+G): dialog accepting `LINE` or `LINE:COLUMN`; clamps out-of-range values;
scrolls to centre.
6.7 **Status bar** (left to right): `Ln 41, Col 7` (and `(N selected)` / `N lines` when selecting) ·
language button · indentation button (e.g. `Spaces: 4`) · encoding button · line-ending button · zoom %. Buttons open
the respective pickers (§7.1, §9). Column counts characters, not bytes, with tabs as one column.

## 7. Code features

7.1 **Language detection**: `GtkSource.LanguageManager.guess_language(filename, content_type)`;
if that yields nothing, `core/language.rs` tries the exact file name (`Makefile`, `Dockerfile`,
`CMakeLists.txt`, `.bashrc`, `.gitignore`...), then the shebang (`#!/usr/bin/env python3` → python3),
then an editor modeline (`vim: ft=` / `-*- mode: -*-`) in the first/last five lines. Untitled tabs
re-detect from content on first save and when the first line is a shebang. A status-bar picker (with
search) overrides it per tab; "Auto Detect" clears the override. The override persists in the session.
7.2 **Highlighting must cover at least**: Rust, C, C++, C#, Go, Java, Kotlin, Swift, JavaScript,
TypeScript, Python, Ruby, PHP, Lua, Shell/bash, Perl, SQL, HTML, CSS/SCSS, XML, JSON, YAML, TOML,
INI/desktop, Markdown, reStructuredText, LaTeX, Dockerfile, Makefile, CMake, diff/patch, Log.
(All are shipped with GtkSourceView 5; the requirement is that the acceptance fixtures in
`tests/fixtures/highlight/` each resolve to the expected language id.)
7.3 **Style scheme** follows the app theme; users can choose from installed schemes in Preferences.
7.4 **Toggle comment** (Ctrl+/ ; Ctrl+Shift+A for block):
 - Line comment applies to every line touched by the selection (or the cursor line). If **all
   non-blank touched lines** are already commented, uncomment; otherwise comment all.
 - The marker is inserted at the **minimum indentation column** of the touched non-blank lines,
   followed by one space; blank lines are skipped. Uncommenting removes the marker and exactly one
   following space if present, tolerating leading whitespace.
 - Languages with only block comments (HTML, CSS, XML, Markdown) wrap each line individually
   (`<!-- … -->`) when the line-comment command is used; the block command wraps the whole
   selection, or the cursor line if nothing is selected, and unwraps if the selection is already
   wrapped.
 - Marker source: language metadata; override table for gaps (e.g. YAML `#`, TOML `#`, JSON none —
   JSON/plain text report "This language has no comment syntax" in a toast and do nothing).
 - The whole operation is one undo step; selection is preserved and shifted.
 Pure function: `toggle_line_comment(lines, marker) -> edits` and `toggle_block_comment(text, start,
 end)` in `core/comment.rs` with table-driven tests (nested indentation, mixed state, blank lines,
 CRLF-free input, trailing space, `//!` doc-comment lines left alone when uncommenting `//`).
7.5 **Line operations** (Edit ▸ Lines; all single undo steps, operate on selection or current
line): Duplicate (Ctrl+Shift+D), Move up/down (Alt+↑/↓), Delete line (Ctrl+Shift+K), Join lines
(Ctrl+J, collapses the newline plus following indentation to one space), Sort ascending/descending
(case-sensitive and case-insensitive variants, numeric-aware optional), Remove duplicate lines,
Remove blank lines, Reverse lines, Trim trailing whitespace, Trim leading whitespace.
7.6 **Case**: UPPER, lower, Title Case, swap case on the selection; no selection → the word under
the cursor.
7.7 **Insert**: date/time (F5, format from preferences, default ISO 8601 local).
7.8 **Markdown/YAML specifics**: highlighting only in v1. Enter on a Markdown list item continues
the list (`- `, `* `, `1. `) and Enter on an empty item ends it (should, M4 stretch).

## 8. Find and replace

8.1 **Find bar** at the top of the editor area (Ctrl+F opens/focuses and fills from selection;
Ctrl+H opens with the replace row; Esc closes and returns focus to the editor). F3 / Shift+F3 and
Enter / Shift+Enter move to next/previous match, wrapping with a "Wrapped" hint.
8.2 **Options** (toggle buttons, persisted): Match case (`Aa`), Whole word (`ab`), Regular expression
(`.*`). Search-in scope: **This tab** (default) · **Selection** · **All open tabs**.
8.3 **Live feedback**: all matches highlighted in the editor as the query is typed (incremental,
debounced 100 ms); a counter `3 of 17` / `No results`; the entry gets the `error` style and an inline
message for an invalid regex, with the engine's error text; empty query clears highlights.
8.4 **Regex flavour**: GLib PCRE2 (`GRegex`): `^`/`$` match at line boundaries; `.` does not match
newline; multi-line patterns work (`\n` allowed); case-insensitive via the Match case toggle.
Tooltip states this. Zero-length matches are handled (no infinite loops; `^` replaced once per line).
8.5 **Replace**: **Replace** replaces the current match and advances; **Replace All** replaces
every match in scope as one undo step and reports `Replaced N`. Plain mode treats the replacement
literally. Regex mode supports `\0` (whole match), `\1`–`\9`, `\n`, `\t`, `\\`; `$1`/`${1}`/`$0`
are accepted and translated (`$$` is a literal `$`). Translation lives in `core/search.rs` with
tests; the translated string is what is passed to GtkSource.
8.6 **All open tabs**: results appear in a collapsible list under the bar (tab name, line number,
line preview with the match emphasised); clicking activates that tab and selects the match.
Replace All in this scope applies to every listed tab, each its own undo step, and requires a
confirmation showing the counts per tab.
8.7 **Find in selection** is limited to the selection captured when the scope is chosen.
8.8 Searching never blocks the UI on large buffers: counting is asynchronous (GtkSource does this);
for buffers over 50 MB counting is off (§3.6) but find-next still works.
8.9 The last 20 queries and replacements are available from a history dropdown on each entry and
persisted in settings (privacy toggle "Remember search history", default on).

## 9. Encodings and line endings

9.1 **Supported encodings**: UTF-8 (with or without BOM), UTF-16 LE/BE and UTF-32 LE/BE (all
encoded and decoded by hand in `core`, because `encoding_rs` cannot *encode* UTF-16 and does not
know UTF-32), plus every single- and multi-byte encoding `encoding_rs` can both decode and encode:
Windows-1250…1258, ISO-8859-2/3/4/5/6/7/8/10/13/14/15/16, KOI8-R, KOI8-U, IBM866, macintosh,
windows-874, GBK, GB18030, Big5, EUC-JP, Shift_JIS, EUC-KR. ISO-8859-1 is offered as a label for
Windows-1252's identical-for-text behaviour only if the round trip test proves it. ISO-2022-JP and
UTF-16 "replacement" encodings are decode-only in `encoding_rs`, so they are not offered for saving.
Out of scope for v1: CP437, CP850, EBCDIC and other legacy code pages (a later iconv fallback).
9.2 **Detection on open** (in order): (1) BOM → that Unicode encoding; (2) bytes valid as UTF-8 →
UTF-8 (a file of pure ASCII is UTF-8); (3) UTF-16 without BOM when ≥ 30% of bytes in the first
8 KiB are NUL in an alternating pattern; (4) `chardetng` over the first 64 KiB, with the user's
locale as the TLD hint; (5) fallback Windows-1252. Result carries a `confidence` (`Certain` for
BOM/valid UTF-8, `Guessed` otherwise). Guessed detections show the encoding button highlighted and
a one-time banner "Opened as Windows-1251 (guess) — Reopen with encoding…".
9.3 **Status-bar encoding menu** has two sections: *Reopen with encoding…* (re-reads the file from
disk using the chosen encoding; if the tab is dirty, confirm because unsaved edits would be
discarded; not available for untitled tabs — their text is already Unicode) and *Save with
encoding…* (changes the tab's target encoding without touching the text; marks the tab dirty).
These two are never combined into one ambiguous "change encoding" item.
9.4 **Convert/save with lossy characters**: before writing (and in the menu preview) the tab text
is encoded; if any character is unrepresentable (`encoding_rs` reports replacements), show a dialog
listing the count and the first 10 characters with line numbers and offer **Save anyway
(replace with `?`/numeric references as the encoder does)** / **Choose another encoding** /
**Cancel**. Saving never silently loses characters. A "Check conversion" menu item runs the same
check without saving and jumps to the first offending character.
9.5 **UTF-8 BOM** is a separate toggle in the encoding menu ("UTF-8" vs "UTF-8 with BOM"); other
Unicode encodings keep their BOM as detected, and new files default to UTF-8 without BOM
(preference changes the default).
9.6 **Invalid bytes** in a file detected as UTF-8-by-BOM or forced-UTF-8 decode with U+FFFD and
mark the tab with a banner "Some bytes were not valid UTF-8 and were replaced; saving will write
the replacement characters." Saving over such a file requires confirmation once per tab.
9.7 **Line endings**: detect LF / CRLF / CR; if more than one style occurs, label "Mixed (mostly
CRLF)" and ask on save: **Convert all to <mostly>** / **Keep as is (normalises to <mostly>)**
(v1 cannot preserve mixed endings; the first option is the default and this is documented). Status-bar
menu converts the target ending (marks tab dirty). New files default to LF (preference; platform
default is not used).
9.8 **Core API** (all pure, in `core/encoding.rs` and `core/line_ending.rs`):
```rust
pub struct Decoded { pub text: String, pub encoding: EncodingId, pub bom: bool,
                     pub confidence: Confidence, pub had_errors: bool,
                     pub line_ending: LineEnding, pub mixed_endings: bool }
pub fn decode(bytes: &[u8], hint: Option<EncodingId>) -> Decoded;   // hint = forced encoding
pub struct Encoded { pub bytes: Vec<u8>, pub unmappable: Vec<Unmappable> }
pub fn encode(text: &str, enc: EncodingId, bom: bool, ending: LineEnding) -> Encoded;
pub fn check_convertible(text: &str, enc: EncodingId) -> Vec<Unmappable>; // line, column, char
```
The text returned by `decode` is `\n`-normalised; `encode` applies the ending.
Property tests: `encode(decode(b)) == b` for every fixture without errors; for any `String` s,
`decode(encode(s, UTF-8/16/32)) == s`.

## 10. Preferences

`~/.config/notey/settings.json` (versioned, unknown keys preserved). `adw::PreferencesWindow` with
pages **Editor** (font, size, **indent width (default 4)**, spaces/tabs, auto-indent, wrap, line numbers, current
line, whitespace, margin, auto-close brackets, cut-line-when-empty, ensure final newline),
**Appearance** (theme, style scheme, minimap), **Files** (default encoding, default line ending,
large-file thresholds, reload-on-change behaviour, recent size), **Session** (remember tabs,
clear saved session, search history). Changes apply live to all open tabs.

## 11. Keyboard shortcuts

| Action | Keys | Action | Keys |
| --- | --- | --- | --- |
| New tab | Ctrl+T / Ctrl+N | Find | Ctrl+F |
| Open | Ctrl+O | Replace | Ctrl+H |
| Save / Save As / Save All | Ctrl+S / Ctrl+Shift+S / Ctrl+Alt+S | Next / previous match | F3 / Shift+F3 |
| Close tab | Ctrl+W | Go to line | Ctrl+G |
| Reopen closed tab | Ctrl+Shift+T | Toggle comment | Ctrl+/ |
| Quit | Ctrl+Q | Block comment | Ctrl+Shift+A |
| Next / prev tab | Ctrl+Tab, Ctrl+PgDn / Ctrl+Shift+Tab, Ctrl+PgUp | Duplicate / delete line | Ctrl+Shift+D / Ctrl+Shift+K |
| Jump to tab N | Alt+1…9 | Move line | Alt+↑ / Alt+↓ |
| Word wrap | Alt+Z | Join lines | Ctrl+J |
| Zoom in / out / reset | Ctrl+= / Ctrl+- / Ctrl+0 | Reload from disk | Ctrl+R |
| Shortcuts window | Ctrl+? | Insert date/time | F5 |

All actions are `gio::SimpleAction`s on the window/app with `set_accels_for_action`, so they appear in
the Keyboard Shortcuts window and can be driven in tests.

## 12. Accessibility and platform integration

Every icon-only button has an accessible label and tooltip; find-bar results announce the count; focus
order is header → tabs → editor → find bar → status bar; high-contrast and large-text settings are
respected (no hard-coded colours outside style schemes). The app is a GNOME Notepad-class
`TextEditor`: desktop file `Categories=GTK;GNOME;Utility;TextEditor;`, `MimeType=text/plain;
text/markdown;text/x-*;application/json;application/x-yaml;...`, metainfo, symbolic and full-colour icon.

## 13. Error handling rules

- No panics on user data. Every I/O error is shown with the file name and the OS message.
- A failure in the session store never blocks editing: it logs, shows a single persistent warning
  banner ("Can't save session — your unsaved tabs may be lost on quit") and retries on the next flush.
- Operations on a closed tab/document are no-ops (async callbacks hold weak references).

## 14. Test plan

| Layer | What | How |
| --- | --- | --- |
| `core::encoding` | detection order, BOM handling, UTF-16 without BOM, legacy fixtures (cp1251, cp1252, Shift_JIS, GBK, KOI8-R), lossy reporting, round trips | unit + property tests, fixtures in `tests/fixtures/encodings/` |
| `core::line_ending` | LF/CRLF/CR/mixed detection, normalise/apply, empty and no-trailing-newline files | unit |
| `core::comment` | table-driven per §7.4 across `//`, `#`, `--`, `<!-- -->`, `/* */` | unit |
| `core::text_ops` | sort variants, dedupe, join, case, indent/unindent, move/duplicate at buffer edges | unit |
| `core::search` | `$N`→`\N` translation, escapes, invalid-pattern errors, cross-tab scan results | unit |
| `core::session` + `application::session_store` | manifest round trip, atomic write (simulated crash leaves `.bak`/previous intact), corrupt manifest fallback, missing buffer file, orphan cleanup, version-too-new, disk-changed reconciliation, debounce coalescing (fake clock) | integration with `tempfile` and `NOTEY_SESSION_DIR` |
| `core::language` | extension, file name, shebang, modeline mapping | unit |
| `application::file_io` | atomic save keeps permissions, save failure leaves original, size tiers, binary guard | integration |
| UI | actions registered; new/close/reopen tab; dirty indicator; find bar state machine; status bar text; session restore of a seeded directory; skips when no display | `tests/headless_gtk.rs` |
| Manual acceptance | kill -9 mid-typing then relaunch; huge file; open the same file from two instances; encoding menu on a cp1251 file; light/dark switch | checklist in `docs/acceptance.md` (written in M7) |

`cargo test` and `cargo clippy -- -D warnings` must pass at every milestone; `scripts/check-core-pure.sh`
runs in CI.
