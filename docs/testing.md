# Testing

One command runs everything CI runs:

```bash
scripts/ci.sh            # core purity, clippy -D warnings, every test, release build
scripts/ci.sh --quick    # skip clippy and the release build while developing
```

Without a display the script wraps cargo in `xvfb-run -a` (`sudo apt install xvfb`). It sets
`NOTEY_REQUIRE_DISPLAY=1` so a missing display fails instead of silently skipping tests, and
`NOTEY_STRICT_WARNINGS=1` so a GLib/GTK warning or critical fails the window scenario that caused it.

## Before you push

```bash
scripts/install-hooks.sh        # once per clone: turns on the pre-push hook
```

The **pre-push hook** runs `scripts/ci.sh --quick` (core purity and every test, under `xvfb-run` when
there is no display) and stops the push if anything fails. `git push --no-verify` skips it, and it skips
itself when the newest commit carries the no-build marker (see "Skipping a build" below).
`scripts/install-hooks.sh --remove` turns it off.

**One Rust version everywhere.** `rust-toolchain.toml` pins the toolchain, so a clippy lint added in a
newer release cannot fail CI while your machine (on an older Rust) is green. rustup picks it up
automatically. With an older rustup (before 1.28) install it once:
`rustup toolchain install $(sed -n 's/^channel *= *"\(.*\)"/\1/p' rust-toolchain.toml) --profile minimal --component clippy`.
CI reads the same file. To move to a newer Rust, change `channel`, run `scripts/ci.sh`, fix what is new,
and commit both together. (`rust-version` in `Cargo.toml` is the *minimum* the code supports and is
separate from this.)

Whatever passes `scripts/ci.sh` here should pass on GitHub except for timing: the runner is slower, so
tests wait for conditions instead of sleeping fixed times.

## Continuous integration (GitHub Actions)

| Workflow | Runs on | Does |
| --- | --- | --- |
| `.github/workflows/test.yml` | every push to a branch other than `main`, every pull request | `scripts/ci.sh` on `ubuntu-24.04`; uploads `target/ci-test.log` |
| `.github/workflows/deb.yml` | pushes to `main` and `v*` tags | `scripts/ci.sh`, then builds `notey_<version>_amd64.deb` and uploads it as an artifact; a `v*` tag also publishes a GitHub release with the package (the tag must equal the version in `Cargo.toml`) |

### Skipping a build

A push triggers a build, except for docs-only changes (`docs/**` and `*.md` never start the package
workflow). For anything else that does not change behaviour, such as a spelling fix, a reworded string
or a comment, put **`[skip build]`** anywhere in the commit message:

```
Fix a typo in the shortcuts list [skip build]
```

- It applies to the **newest commit of the push**: push three commits and only the last one's message is
  read. To skip a batch, mark the last commit; to build it, don't.
- A **tag** (`v*`) always builds, whatever the commit says, so a release is never skipped by accident.
- To build a commit that was skipped, open **Actions**, choose the workflow and use **Run workflow**
  (pick the branch). Pull requests always run their checks.
- GitHub's own `[skip ci]` also works, but it skips *every* workflow including a tag's package build, so
  prefer `[skip build]`.

Both install `libgtk-4-dev libadwaita-1-dev libgtksourceview-5-dev xvfb` and the Rust version pinned in
`rust-toolchain.toml` (with clippy). `Cargo.lock` must be committed because the scripts build with `--locked`.

## Layers

| Layer | Where | What it proves | Display |
| --- | --- | --- | --- |
| Unit | `src/**` (`#[cfg(test)]`) | Every rule of the GTK-free core: encodings, line endings, comment toggling, text operations, search queries, session model and store, the single writer thread, settings, CLI arguments, quick-bar reordering | no |
| Encoding fixtures | `tests/encoding_fixtures.rs`, `tests/fixtures/encodings/` | Real files in 13 encodings and line-ending styles are detected, decoded and saved back byte for byte; lossy conversion is reported | no |
| Properties | `tests/properties.rs` (proptest) | Round trips and invariants hold for arbitrary input: encodings, line endings, comment toggle twice, sort/dedupe idempotence, indent/unindent, session JSON, drag reordering is a permutation, nothing panics on garbage | no |
| Window scenarios | `tests/headless_gtk/` | The real window driven through its actions: restore after quit and crash, saving, external changes, comments and line operations, find and replace, status bar, settings, quick bar, preferences, menus and shortcuts all wired to real actions | yes |
| Process smoke | `tests/process_smoke.rs` | The real binary: opens files from the command line, survives `kill -9` and restores, shuts down cleanly on `SIGTERM`, sets aside a corrupt session, leaves a newer-version session alone | yes |

## Window scenarios

GTK may only be initialised once per process, so `tests/headless_gtk` has its own `main` and runs
named scenarios one after another, each in its own temporary profile:

```bash
xvfb-run -a cargo test --test headless_gtk                  # all of them
xvfb-run -a cargo test --test headless_gtk -- find          # names containing "find"
NOTEY_STRICT_WARNINGS=1 xvfb-run -a cargo test --test headless_gtk
```

Output is one `scenario <name> ... ok` line each, a failure message under any that fail, and a
summary line; the exit code is non-zero if any failed. Names start with the area: `session_`,
`files_`, `editing_`, `find_`, `ui_`. Add a scenario by writing a `scenario("name", |env| { ... })`
entry in the matching module; `support.rs` has helpers (`open_ok`, `save`, `act`, `block_on`,
`pump_for`, `quit`, ...).

## What is covered, by spec section

| Spec | Covered by |
| --- | --- |
| §3 File operations | `files_*` scenarios, `encoding_fixtures`, `file_io` unit tests (atomic save, symlinks, permissions, size tiers, binary guard) |
| §4 Tabs | `session_untitled_*`, `session_closed_*`, `files_tabs_with_the_same_name_*`, `ui_pinning_*`, `ui_tab_navigation_*` |
| §5 Session | `session_*` scenarios, `session_store` and writer unit tests, `process_smoke` (kill -9, corrupt, newer version) |
| §6 Editing | `editing_*` scenarios, `text_ops` unit and property tests |
| §7 Code features | `editing_comment_*`, `ui_language_*`, `comment` unit and property tests |
| §8 Find and replace | `find_*` scenarios |
| §9 Encodings | `encoding_fixtures`, `properties`, `files_reopen_*`, `files_changing_line_ending_*`, `ui_status_bar_shows_bom_*` |
| §10 Preferences | `ui_preferences_*`, `ui_settings_apply_*`, settings unit tests |
| §11 Shortcuts | `ui_every_shortcut_names_a_real_action_and_no_key_is_used_twice`, `ui_tab_view_leaves_*` |

## Known gaps (need a person or a real pointer)

- Dialog-driven flows that wait for a click: the unsaved-tab close prompt, the lossy-save warning, the
  binary and very-large-file prompts, Rename. The checks behind them are unit tested; `docs/acceptance.md`
  lists the manual steps.
- Pointer gestures: an actual mouse drag of a quick-action row, tab dragging, a real file drop from a file
  manager. The code those gestures call (`move_entry`, `open_dropped`) is tested.
- Keyboard typing into the editor (auto-closing brackets, cut/copy of the line when nothing is selected).
- Light/dark switching by the desktop, and rendering (screenshots are checked by eye).

## Bugs these tests found

Recorded because they are the reason several tests exist: tab-view shortcuts stealing Ctrl+Home/End;
`SearchContext::replace_all` in the bindings panicking on "nothing to replace" (an invalid regex or
`$` would have crashed the app); GtkSourceView skipping zero-length regex matches (`^`, `$`); a
`RefCell` double borrow when searching all tabs; read-only tabs accepting line edits; same-named tabs
not showing their folder on the first of the pair; re-opening a file not refreshing "recent files"; and
the GTK recent-files registration writing to the real recently-used list from tests.
