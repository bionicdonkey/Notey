# Manual acceptance checklist

Run with a throwaway profile: `NOTEY_SESSION_DIR=$(mktemp -d) NOTEY_CONFIG_DIR=$(mktemp -d) cargo run`.

## Session
- [ ] Type in an untitled tab, wait two seconds, `kill -9` the process, relaunch: the text is back and the tab is marked unsaved
- [ ] Edit an opened file without saving, quit with Ctrl+Q (no prompt), relaunch: the edit is back, the file on disk is unchanged
- [ ] Edit a file, quit, change the file in another editor, relaunch: the banner offers Reload / Keep Mine and neither version is lost
- [ ] Close an unsaved tab with Discard, press Ctrl+Shift+T: the tab returns with its text
- [ ] Pin a tab, scroll, move the cursor, relaunch: pin, cursor and scroll position are restored

## Files
- [ ] Open the same file twice (once from the recent list): one tab
- [ ] Save a CRLF file with a UTF-8 BOM unchanged: `cmp` against the original reports no difference
- [ ] Save through a symlink: the link survives
- [ ] Open a >5 MB file (large notice, no highlighting), a binary file (prompt, read-only), a read-only file (indicator)
- [ ] Edit an open, clean file elsewhere: the tab reloads with a toast; edit an unsaved one: the banner appears
- [ ] `notey +10 file.txt` and `notey file.txt:10` jump to line 10; a second `notey other.txt` adds a tab to the running window

## Editing and code
- [ ] Ctrl+/ on mixed commented and plain lines, in Rust, Python, HTML, CSS and JSON (JSON shows "no comment syntax")
- [ ] Indent width 2 in Preferences changes tab stops in new tabs; per-tab override from the status bar
- [ ] Sort, dedupe, move, join, duplicate and delete lines, each undone with one Ctrl+Z
- [ ] Ctrl+X with nothing selected cuts the line

## Find and replace
- [ ] Plain, case, whole word; regex with `(\w+)@(\w+)` → `\2 at \1` and `$2 at $1`; an invalid pattern shows an inline error
- [ ] Selection scope only touches the selection; all-tabs scope lists hits across tabs and Replace All confirms with per-tab counts

## Encodings
- [ ] A Windows-1251 file: status bar shows the guess; Reopen with Encoding fixes it; save keeps the bytes
- [ ] Save a document containing “日本” as Windows-1252: the dialog lists the characters and positions
- [ ] Switch the line ending to CRLF and save; open a mixed file and save: the mixed-endings prompt appears

## Appearance
- [ ] Switch the system between light and dark: editor colours follow
- [ ] Narrow the window to phone width: the header, tabs and status bar remain usable
