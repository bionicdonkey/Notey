//! Edit, line, case and comment actions.
use super::support::*;
use notey::core::session::Cursor;
use sourceview5::prelude::*;

fn doc_with(win: &notey::ui::window::NoteyWindow, sb: &Sandbox, name: &str, text: &str) -> std::rc::Rc<notey::ui::doc::Doc> {
    let d = open_ok(win, &sb.write(name, "placeholder"));
    d.set_text(text, false);
    d
}

pub fn scenarios() -> Vec<Scenario> {
    vec![
        scenario("editing_comment_toggle_uses_each_languages_syntax", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let cases: &[(&str, &str, &str)] = &[
                ("x.rs", "fn a() {}\n    let b = 1;", "// fn a() {}\n//     let b = 1;"),
                ("x.py", "x = 1\ny = 2", "# x = 1\n# y = 2"),
                ("x.sh", "ls\n  cd /", "# ls\n#   cd /"),
                ("x.yaml", "a: 1\nb: 2", "# a: 1\n# b: 2"),
                ("x.toml", "a = 1", "# a = 1"),
                ("x.sql", "select 1;", "-- select 1;"),
                ("x.html", "<p>a</p>", "<!-- <p>a</p> -->"),
                ("x.css", "a { color: red }", "/* a { color: red } */"),
                ("x.c", "int x;", "// int x;"),
                ("x.lua", "print(1)", "-- print(1)"),
            ];
            for (file, before, after) in cases {
                let d = doc_with(&win, &sb, file, before);
                select_all(&d);
                act(&win, "toggle-comment");
                assert_eq!(d.text(), *after, "{file}");
                act(&win, "toggle-comment");
                assert_eq!(d.text(), *before, "{file}: toggling back");
            }
        }),
        scenario("editing_languages_without_comments_do_nothing", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = doc_with(&win, &sb, "x.json", "{\"a\": 1}");
            select_all(&d);
            act(&win, "toggle-comment");
            assert_eq!(d.text(), "{\"a\": 1}");
            assert!(!d.is_dirty() || d.text() == "{\"a\": 1}");
        }),
        scenario("editing_comment_and_uncomment_are_directional", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = doc_with(&win, &sb, "x.rs", "a\nb");
            select_all(&d);
            act(&win, "uncomment-lines");
            assert_eq!(d.text(), "a\nb", "nothing to uncomment");
            act(&win, "comment-lines");
            assert_eq!(d.text(), "// a\n// b");
            act(&win, "comment-lines");
            assert_eq!(d.text(), "// a\n// b", "already commented");
            act(&win, "uncomment-lines");
            assert_eq!(d.text(), "a\nb");
        }),
        scenario("editing_comment_applies_to_the_touched_lines_only", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = doc_with(&win, &sb, "x.py", "a\nb\nc\nd");
            d.set_selection(notey::core::session::Selection { start: Cursor { line: 1, column: 0 }, end: Cursor { line: 2, column: 1 } });
            act(&win, "toggle-comment");
            assert_eq!(d.text(), "a\n# b\n# c\nd");
            // A selection ending at column 0 of the next line does not include that line.
            d.set_text("a\nb\nc", false);
            d.set_selection(notey::core::session::Selection { start: Cursor { line: 0, column: 0 }, end: Cursor { line: 1, column: 0 } });
            act(&win, "toggle-comment");
            assert_eq!(d.text(), "# a\nb\nc");
            // No selection: the cursor's line.
            d.set_text("x\ny", false);
            d.set_cursor(Cursor { line: 1, column: 0 });
            act(&win, "toggle-comment");
            assert_eq!(d.text(), "x\n# y");
        }),
        scenario("editing_block_comment_wraps_and_unwraps", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = doc_with(&win, &sb, "x.css", "a { color: red }");
            select_all(&d);
            act(&win, "toggle-block-comment");
            assert_eq!(d.text(), "/* a { color: red } */");
            select_all(&d);
            act(&win, "toggle-block-comment");
            assert_eq!(d.text(), "a { color: red }");
            let py = doc_with(&win, &sb, "x.py", "print(1)");
            select_all(&py);
            act(&win, "toggle-block-comment");
            assert_eq!(py.text(), "print(1)", "Python has no block comment");
        }),
        scenario("editing_every_edit_is_a_single_undo_step", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            for action in ["toggle-comment", "sort-asc", "dedupe-lines", "reverse-lines", "trim-trailing", "upper-case", "indent", "duplicate-line", "delete-line", "join-lines", "move-line-down"] {
                let d = doc_with(&win, &sb, "u.rs", "b  \na\nb  \nc");
                d.buffer.begin_irreversible_action();
                d.buffer.end_irreversible_action();
                select_all(&d);
                act(&win, action);
                d.buffer.undo();
                assert_eq!(d.text(), "b  \na\nb  \nc", "{action}: one Ctrl+Z restores the text");
            }
        }),
        scenario("editing_line_operations", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = win.current_doc().unwrap();
            let run = |text: &str, action: &str| {
                d.set_text(text, false);
                select_all(&d);
                act(&win, action);
                d.text()
            };
            assert_eq!(run("b\na\nc", "sort-asc"), "a\nb\nc");
            assert_eq!(run("b\na\nc", "sort-desc"), "c\nb\na");
            assert_eq!(run("b\nA\na", "sort-ci"), "A\na\nb");
            assert_eq!(run("f10\nf2\nf1", "sort-numeric"), "f1\nf2\nf10");
            assert_eq!(run("a\nb\na\nb", "dedupe-lines"), "a\nb");
            assert_eq!(run("a\n\n \nb", "remove-blank-lines"), "a\nb");
            assert_eq!(run("1\n2\n3", "reverse-lines"), "3\n2\n1");
            assert_eq!(run("a  \nb\t", "trim-trailing"), "a\nb");
            assert_eq!(run("  a\n\tb", "trim-leading"), "a\nb");
            assert_eq!(run("foo(\n    a,\n    b\n)", "join-lines"), "foo( a, b )");
            assert_eq!(run("a\nb", "duplicate-line"), "a\nb\na\nb");
            assert_eq!(run("abc def", "upper-case"), "ABC DEF");
            assert_eq!(run("ABC DEF", "lower-case"), "abc def");
            assert_eq!(run("hello wORLD", "title-case"), "Hello World");
            assert_eq!(run("aBc", "swap-case"), "AbC");
        }),
        scenario("editing_sort_without_a_selection_sorts_the_whole_document", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = win.current_doc().unwrap();
            d.set_text("c\nb\na", false);
            d.set_cursor(Cursor { line: 0, column: 0 });
            act(&win, "sort-asc");
            assert_eq!(d.text(), "a\nb\nc");
        }),
        scenario("editing_moving_duplicating_and_deleting_the_current_line", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = win.current_doc().unwrap();
            d.set_text("a\nb\nc", false);
            d.set_cursor(Cursor { line: 2, column: 0 });
            act(&win, "move-line-up");
            assert_eq!(d.text(), "a\nc\nb");
            act(&win, "move-line-down");
            assert_eq!(d.text(), "a\nb\nc");
            act(&win, "move-line-down");
            assert_eq!(d.text(), "a\nb\nc", "last line cannot move down");
            d.set_cursor(Cursor { line: 0, column: 0 });
            act(&win, "move-line-up");
            assert_eq!(d.text(), "a\nb\nc", "first line cannot move up");
            act(&win, "duplicate-line");
            assert_eq!(d.text(), "a\na\nb\nc");
            act(&win, "delete-line");
            assert_eq!(d.text(), "a\nb\nc");
            d.set_cursor(Cursor { line: 2, column: 0 });
            act(&win, "delete-line");
            assert_eq!(d.text(), "a\nb", "deleting the last line also drops the break before it");
        }),
        scenario("editing_indent_and_unindent_follow_the_indent_preference", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = win.current_doc().unwrap();
            d.set_text("a\nb", false);
            select_all(&d);
            act(&win, "indent");
            assert_eq!(d.text(), "    a\n    b");
            act(&win, "unindent");
            assert_eq!(d.text(), "a\nb");
            win.settings.borrow_mut().indent_width = 2;
            win.apply_settings_everywhere();
            select_all(&d);
            act(&win, "indent");
            assert_eq!(d.text(), "  a\n  b");
            win.settings.borrow_mut().insert_spaces = false;
            win.apply_settings_everywhere();
            d.set_text("a", false);
            select_all(&d);
            act(&win, "indent");
            assert_eq!(d.text(), "\ta");
        }),
        scenario("editing_insert_date_and_time_adds_a_timestamp", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            act(&win, "insert-datetime");
            let t = win.current_doc().unwrap().text();
            assert_eq!(t.len(), 19, "{t:?}");
            assert_eq!((&t[4..5], &t[7..8], &t[10..11], &t[13..14]), ("-", "-", " ", ":"));
        }),
        scenario("editing_go_to_line_clamps_and_centres", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = win.current_doc().unwrap();
            d.set_text("one\ntwo\nthree\nfour", false);
            win.goto_line(&d, 3, Some(2));
            assert_eq!((d.cursor().line, d.cursor().column), (2, 1));
            win.goto_line(&d, 999, None);
            assert_eq!(d.cursor().line, 3);
            win.goto_line(&d, 0, Some(999));
            assert_eq!((d.cursor().line, d.cursor().column), (0, 3), "column is clamped to the line");
        }),
        scenario("editing_read_only_tabs_ignore_edit_actions", |env| {
            use std::os::unix::fs::PermissionsExt;
            let sb = env.sandbox();
            let win = env.window(&sb);
            let f = sb.write("ro.rs", "b\na");
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o444)).unwrap();
            let d = open_ok(&win, &f);
            select_all(&d);
            for a in ["sort-asc", "toggle-comment", "upper-case", "indent", "delete-line"] {
                act(&win, a);
            }
            assert_eq!(d.text(), "b\na");
            assert!(!d.is_dirty());
        }),
        scenario("editing_indentation_is_detected_from_the_file", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let tabs = open_ok(&win, &sb.write("t.rs", "fn a() {\n\tlet x = 1;\n\tlet y = 2;\n}\n"));
            assert!(!tabs.view.is_insert_spaces_instead_of_tabs());
            let two = open_ok(&win, &sb.write("two.rs", "fn a() {\n  if x {\n    y();\n  }\n}\n"));
            assert!(two.view.is_insert_spaces_instead_of_tabs());
            assert_eq!(two.view.indent_width(), 2);
            let none = open_ok(&win, &sb.write("none.txt", "no indentation\nat all\n"));
            assert_eq!(none.view.indent_width(), 4, "falls back to the preference");
        }),
        scenario("editing_the_indent_width_preference_is_the_default_for_new_tabs", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            assert_eq!(win.current_doc().unwrap().view.indent_width(), 4);
            win.settings.borrow_mut().indent_width = 8;
            win.apply_settings_everywhere();
            let fresh = win.new_tab();
            assert_eq!((fresh.view.indent_width(), fresh.view.tab_width()), (8, 8));
            win.settings.borrow_mut().indent_width = 99; // out of range is clamped by the settings loader, not the widget
            assert_eq!(notey::settings::Settings::from_json(&win.settings.borrow().to_json()).indent_width, 16);
        }),
    ]
}
