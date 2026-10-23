# Notey: plan

A native Linux text editor in the vein of Windows Notepad / Notepad++-lite, built with Rust, GTK 4,
libadwaita and GtkSourceView 5. It is the sibling of Zorin Explorer (`/storage/nvmep2/repos/Gitea/FileExplorer`)
and follows the same structure, toolchain and conventions. The behaviour is specified in
`docs/spec.md`; this document says what we build, in what order, and why.

Names: short name **Notey** (binary `notey`, crate `notey`, app id `com.zorin.Notey`, window and
menu text); full name **NoteyMcNoteface** (About dialog, desktop file `Name=`, metainfo, README title).

## Goals

1. Tabbed editing where **nothing is ever lost**: every tab, including untitled and unsaved ones,
   survives quitting, crashes and reboots, and comes back exactly as it was (text, cursor, scroll,
   undo-independent).
2. Standard file operations done properly (open many, save, save as, recent, reload on external
   change, drag and drop, command-line files).
3. Standard text operations plus a small set of code-friendly ones (comment/uncomment, indent,
   line operations).
4. Syntax highlighting selected automatically from file name/content, overridable per tab.
5. Find and replace in plain text and regex, in the current tab or across all open tabs.
6. First-class encoding handling: detect on open, show, reopen with another encoding, save/convert
   to another, with explicit warning when conversion would lose characters. Same for line endings.
7. Looks native on GNOME/Zorin: libadwaita, follows dark/light, standard shortcuts.

## Non-goals (v1)

Rich text or any formatting beyond syntax highlighting; plugins; LSP/completion; split panes;
project tree; git integration; remote editing beyond what GIO gives for free; macOS/Windows ports
(the core layer is kept GTK-free so a port stays possible, but we do not build a shared spec
apparatus like the File Manager's `spec/` directory).

## Decisions and rationale

| Decision | Why | Cost / alternative rejected |
| --- | --- | --- |
| **GtkSourceView 5** (`sourceview5` 0.8 crate, library 5.12 on this machine) for the editor widget | Highlighting for ~300 languages, language guessing, undo/redo, bracket matching, line numbers, minimap (`Map`), and a search engine with regex and match highlighting. Writing our own would be months of work. | Adds a system dependency (`libgtksourceview-5-dev`). Rejected: a hand-rolled highlighter. |
| Crate versions pinned to the File Manager's generation: `gtk4` 0.8, `libadwaita` 0.6, `gio`/`glib` 0.19 | Matches the installed GTK 4.14 / libadwaita 1.5 and the sibling project; `sourceview5` 0.8 targets gtk4 0.8. | Newer binding generations would need newer system libraries. |
| **Encoding in pure Rust** (`encoding_rs` + `chardetng`), not GtkSource's `FileLoader` | Keeps decode/encode/detect in the GTK-free `core`, so it is unit-testable without a display, reports lossy conversion precisely (we need that for the convert-with-warning feature), and handles BOMs and line endings in one place. | `encoding_rs` implements the WHATWG set only: no UTF-32 (we add it by hand), no CP437/CP850/EBCDIC. Documented limitation. A later iconv fallback is possible. |
| Buffer always holds `\n`-only text; original line ending and encoding are document metadata applied on save | GtkTextBuffer and every text operation stay simple; conversion becomes a metadata change. | Mixed-ending files are normalised on save, so we flag them and ask. |
| **Session snapshots are separate from saving.** Autosave never writes the user's file | A user must be able to type in a file-backed tab, quit, and not have the file silently modified. Matches Windows 11 Notepad. | Needs its own store (below). An opt-in "also autosave to file" setting is a later add-on. |
| Quitting never prompts; closing a single dirty tab does | The session already preserves everything on quit. Explicitly discarding a tab is the only way to lose text, so that is where we confirm. | Differs from classic editors; stated in the README. |
| Single-instance via `GApplication` (`HANDLES_OPEN`) | Second `notey file.txt` opens a tab in the running window; one process owns the session directory, which removes concurrent-write problems. | None. |
| Settings in `~/.config/notey/settings.json`, not GSettings | No schema install/compile step for `cargo run`. The File Manager's `settings.rs` is also serde JSON. | Not visible to dconf tooling. |
| Comment toggling reads the comment markers from GtkSource language metadata (`line-comment-start`, `block-comment-start/end`) | The language definitions already contain them for most languages; no table to maintain. Logic itself is pure functions in `core`. | Languages lacking metadata get a small override table in `core/comment.rs`. |
| Regex search through GtkSource `SearchContext` | Highlights all matches, counts them asynchronously, handles replace correctly on the live buffer with undo. | Its regex flavour is GLib's PCRE2 (not Rust `regex`): `\1` backreferences and lookarounds work; the replacement syntax is `\1`/`\0`. We also accept `$1`/`${1}` and translate. Cross-tab search uses the same flavour via `glib::Regex` so results agree. |
| Markdown preview and other heavy extras deferred | A rendered preview needs WebKitGTK or a custom renderer, which would triple the dependency weight. Highlighting for Markdown/YAML is in v1 as requested. | Listed under "Later". |

## Architecture

Same layering as Zorin Explorer: a GTK-free `core` that is unit-tested, an `application` layer for
I/O and scheduling, a `ui` layer for widgets.

```
src/
  main.rs               argument handling (--version), Application setup, CSS
  lib.rs
  settings.rs           serde Settings, load/save
  core/                 no GTK, no GIO
    document.rs         DocumentMeta: id, path, encoding, line_ending, language_override, saved_state
    encoding.rs         detect / decode / encode / convert, BOM, lossy report
    line_ending.rs      detect, normalise, apply
    text_ops.rs         sort/dedupe lines, trim, join, case, indent/unindent, duplicate/move line
    comment.rs          toggle line / block comment on a text range
    search.rs           Query { pattern, regex, case, word }, replacement translation, scan (cross-tab)
    language.rs         extension/filename/shebang -> language id fallbacks
    session.rs          Session / TabRecord structs, versioned JSON, reconciliation rules
    recent.rs           bounded most-recently-used list
  application/
    file_io.rs          async read/write via GIO, size limits, atomic save, cancellation
    session_store.rs    atomic snapshot writes, debounce, orphan cleanup, crash-safe restore
    file_watch.rs       GFileMonitor wrapper, self-write suppression
    lifecycle.rs        flush-on-quit, shutdown ordering
  ui/
    window.rs           NoteyWindow: adw::TabView + TabBar, actions, drag/drop
    editor_page.rs      one tab: sourceview::View, Buffer, Map, change tracking
    header.rs           menu button, new tab, open, save, search toggle
    find_bar.rs         find/replace, options, count, all-tabs results
    status_bar.rs       Ln/Col, selection, language, encoding, line ending, zoom
    dialogs/            encoding picker, go-to-line, doc stats, save-changes, preferences
    shortcuts.rs        accelerators + shortcuts window
resources/              style.css, desktop file, icon, metainfo
packaging/deb/          build-deb.sh (copied from Explorer, adapted)
tests/                  core_*.rs, session_roundtrip.rs, headless_gtk.rs
```

## Feature set

**v1 (the request):** tabs with persistent autosaved session; new/open/save/save as/save all/
close/reopen closed tab/recent; cut/copy/paste/undo/redo/select all; syntax highlighting with
language picker; comment/uncomment; find/replace (plain + regex); encodings with detect, reopen-as,
convert; line-ending detect/convert.

**v1 suggestions I'm including** (cheap, they complement the above): go to line; word wrap
toggle; zoom; status bar; line operations (duplicate, move, delete, join, sort, dedupe); trim
trailing whitespace; case conversion; indent/unindent with tab-vs-spaces setting; auto-indent;
bracket matching; line numbers and current-line highlight; minimap; show whitespace; document
statistics; external-change detection with reload prompt; read-only/large-file/binary-file
guards; drag and drop; pin tab; light/dark/system style scheme; font and size preference.

**Later:** Markdown preview, "autosave to file" option, multi-cursor/column select, file
encoding fallback via iconv, snippets, diff of tab vs disk, session export, split view.

## Milestones

Each milestone ends with `cargo test` green and the app runnable. Core logic is written test-first.

| # | Milestone | Done when |
| --- | --- | --- |
| M0 | **Scaffold**: Cargo project, deps, `NoteyWindow` with one `SourceView` in an `adw::TabView`, CSS, `--version`, desktop file | `cargo run` shows a window; typing works; CI-style `cargo build` clean |
| M1 | **Core**: `encoding`, `line_ending`, `text_ops`, `comment`, `search` translation, `session`, `recent`, `language` with full unit tests | Core tests cover every rule in the spec's encoding, comment and session sections |
| M2 | **Files + tabs**: file_io, open/save/save as/save all, dirty indicator, close prompts, reopen closed, recent, drag/drop, CLI args, single instance, external-change watcher | Spec §3 and §4 acceptance list passes manually and in tests |
| M3 | **Session persistence**: session_store, debounce + periodic + event flush, restore, orphan cleanup, disk-changed reconciliation | Kill -9 during typing loses at most ~1 s of text (§5) |
| M4 | **Editing features**: highlighting + language picker, comment toggle, line/text ops, indent, wrap, zoom, go to line, minimap, status bar | Spec §6, §7 pass |
| M5 | **Find/replace**: find bar, plain/regex/case/word, count, replace one/all, cross-tab results | Spec §8 passes |
| M6 | **Encodings UI**: status-bar encoding/line-ending menus, reopen-as, convert with lossy warning, detection confidence | Spec §9 passes |
| M7 | **Polish + packaging**: preferences, shortcuts window, style scheme, icon, metainfo, deb script + workflow, README | `.deb` builds and installs; README feature list matches reality |

## Risks

- **Session corruption/loss** is the one failure users will not forgive. Mitigation: write-temp,
  fsync, rename; keep previous manifest as `session.json.bak`; per-tab files so one bad file cannot
  take down the rest; restore tolerates missing/corrupt entries (§5.6).
- **Large files** freeze GtkTextView. Mitigation: size tiers that disable highlighting/minimap and
  warn (§3.6).
- **Encoding misdetection** on short legacy files. Mitigation: never silently guess when
  confidence is low: show the detected encoding in the status bar and make reopen-as one click.
- **GtkSource regex differences** from Rust `regex`. Mitigation: documented in the find bar's
  tooltip and the README; invalid patterns show an inline error rather than failing silently.
- **Headless GTK tests** need a display. Mitigation: tests that need GTK skip when no display is
  available (as `tests/headless_gtk.rs` does in Explorer).

## Open questions (defaults chosen, tell me to change)

2. Default indentation: 4 spaces (user-configurable in Preferences, see spec §6.4/§10), auto-detected per file where obvious.
3. Recent files list size: 15.
4. Large-file tiers: >5 MB no highlighting, >50 MB read-only-confirm, >200 MB refuse.

## Status (2026-10-24)

M0 to M7 are implemented. 91 unit tests cover `core` and `application`; `tests/headless_gtk.rs`
drives the real window (restore, crash recovery, save, reopen closed tab, comment and line actions,
indent width, plain and regex replace) and needs a display (`xvfb-run -a cargo test`).

Deviations from the spec, all deliberate:
- Save As has no encoding dropdown (no extra-choice area in `gtk::FileDialog`); use the status bar.
- Local files only. Opening goes through `std::fs` on a worker thread, not GIO, so `sftp://` and similar are not opened.
- `glib` 0.19's `Regex` wrappers assert in debug builds when a regex finds no match; the dev profile switches
  `debug-assertions` off for the `glib` package only (`Cargo.toml`). Release builds are unaffected.
- Markdown preview remains in "Later".
