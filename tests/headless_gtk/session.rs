//! Tabs that keep their text: snapshots, restore, crash recovery, closed tabs.
use super::support::*;
use gtk::prelude::*;
use notey::core::session::Cursor;
use notey::settings::Settings;

pub fn scenarios() -> Vec<Scenario> {
    vec![
        scenario("session_first_run_has_one_untitled_tab", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            assert_eq!(win.docs.borrow().len(), 1);
            let d = win.current_doc().unwrap();
            assert_eq!(d.name(), "Untitled");
            assert!(d.path().is_none() && !d.is_dirty());
        }),
        scenario("session_untitled_tabs_take_the_lowest_free_number", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let second = win.new_tab();
            let third = win.new_tab();
            assert_eq!((second.name().as_str(), third.name().as_str()), ("Untitled 2", "Untitled 3"));
            discard_tab(&win, &second);
            assert_eq!(win.new_tab().name(), "Untitled 2", "the gap is reused");
        }),
        scenario("session_unsaved_text_is_restored_after_quit", |env| {
            let sb = env.sandbox();
            let file = sb.write("a.txt", "hello\r\nworld\r\n");
            let win = env.window(&sb);
            let doc = open_ok(&win, &file);
            append(&doc, "edited");
            let scratch = win.new_tab();
            append(&scratch, "scratch notes");
            quit(&win);
            drop(win);

            let win2 = env.window(&sb);
            let docs = win2.docs_in_order();
            assert_eq!(docs.len(), 2);
            assert_eq!(docs[0].path().as_deref(), Some(std::fs::canonicalize(&file).unwrap().as_path()));
            assert_eq!(docs[0].text(), "hello\nworld\nedited");
            assert!(docs[0].is_dirty());
            assert_eq!(docs[0].state.borrow().line_ending.label(), "CRLF");
            assert_eq!((docs[1].text().as_str(), docs[1].is_dirty(), docs[1].path()), ("scratch notes", true, None));
            assert_eq!(sb.read("a.txt"), b"hello\r\nworld\r\n", "the file itself was never written");
        }),
        scenario("session_text_survives_a_crash_with_no_shutdown", |env| {
            let sb = env.sandbox();
            let w1 = env.window(&sb);
            let typed = w1.current_doc().unwrap();
            append(&typed, "typed before the crash");
            pump_for(1400); // past the one second debounce
            // The first window is abandoned: nothing flushes and nothing saves, as after kill -9.
            let w2 = env.window(&sb);
            let docs = w2.docs_in_order();
            assert_eq!(docs.len(), 1);
            assert_eq!((docs[0].id.as_str(), docs[0].text().as_str(), docs[0].is_dirty()), (typed.id.as_str(), "typed before the crash", true));
        }),
        scenario("session_autosave_never_touches_the_users_file", |env| {
            let sb = env.sandbox();
            let file = sb.write("keep.txt", "original\n");
            let win = env.window(&sb);
            let doc = open_ok(&win, &file);
            append(&doc, "changed in the editor");
            pump_for(1400);
            assert_eq!(sb.read("keep.txt"), b"original\n");
            assert!(sb.buffer_file(&doc.id).exists(), "the edit is in the private session copy");
            win.snapshot_all();
            win.shutdown(false);
            assert_eq!(sb.read("keep.txt"), b"original\n");
        }),
        scenario("session_saving_drops_the_snapshot_and_manifest_reference", |env| {
            let sb = env.sandbox();
            let file = sb.write("s.txt", "one\n");
            let win = env.window(&sb);
            let doc = open_ok(&win, &file);
            append(&doc, "two\n");
            pump_for(1300);
            assert!(sb.buffer_file(&doc.id).exists());
            assert!(save(&win, &doc));
            pump_for(200);
            assert_eq!(sb.read("s.txt"), b"one\ntwo\n");
            assert!(!sb.buffer_file(&doc.id).exists());
            assert!(!doc.is_dirty());
            let manifest = win.build_session();
            assert!(manifest.tabs[0].buffer_file.is_none() && !manifest.tabs[0].dirty);
        }),
        scenario("session_an_emptied_untitled_tab_stops_needing_a_snapshot", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = win.current_doc().unwrap();
            append(&d, "x");
            pump_for(1300);
            assert!(sb.buffer_file(&d.id).exists());
            d.set_text("", false);
            win.snapshot(&d);
            pump_for(150);
            assert!(!sb.buffer_file(&d.id).exists());
        }),
        scenario("session_closed_tab_reopens_with_its_unsaved_text", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let keep = win.current_doc().unwrap();
            let scratch = win.new_tab();
            append(&scratch, "precious notes");
            discard_tab(&win, &scratch);
            assert_eq!(win.docs.borrow().len(), 1);
            assert!(Rc::ptr_eq(&win.current_doc().unwrap(), &keep));
            win.reopen_closed();
            let back = win.current_doc().unwrap();
            assert_eq!(back.text(), "precious notes");
            assert!(back.is_dirty());
            assert!(win.closed.borrow().is_empty(), "reopening consumes the entry");
        }),
        scenario("session_closed_list_is_bounded_and_survives_restart", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            for i in 0..23 {
                let d = win.new_tab();
                append(&d, &format!("note {i}"));
                discard_tab(&win, &d);
            }
            assert_eq!(win.closed.borrow().len(), 20);
            assert!(win.closed.borrow()[0].title.starts_with("Untitled"));
            quit(&win);
            drop(win);
            let win2 = env.window(&sb);
            assert_eq!(win2.closed.borrow().len(), 20);
            win2.reopen_closed();
            assert_eq!(win2.current_doc().unwrap().text(), "note 22", "newest closed tab comes back first");
        }),
        scenario("session_order_pins_and_the_active_tab_are_restored", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let docs: Vec<_> = ["one", "two", "three"].iter().map(|n| open_ok(&win, &sb.write(&format!("{n}.txt"), n))).collect();
            let page = win.page_of(&docs[1]).unwrap();
            win.tab_view.set_page_pinned(&page, true);
            win.tab_view.set_selected_page(&win.page_of(&docs[2]).unwrap());
            let active_before = win.current_doc().unwrap().name();
            let order: Vec<String> = win.docs_in_order().iter().map(|d| d.name()).collect();
            quit(&win);
            drop(win);
            let win2 = env.window(&sb);
            let restored: Vec<String> = win2.docs_in_order().iter().map(|d| d.name()).collect();
            assert_eq!(restored, order);
            assert_eq!(win2.current_doc().unwrap().name(), active_before);
            let pinned: Vec<bool> = win2.docs_in_order().iter().map(|d| win2.page_of(d).unwrap().is_pinned()).collect();
            assert_eq!(pinned.iter().filter(|p| **p).count(), 1);
        }),
        scenario("session_cursor_selection_language_and_encoding_are_restored", |env| {
            let sb = env.sandbox();
            let file = sb.write("notes.txt", "alpha\nbeta\ngamma\ndelta\n");
            let win = env.window(&sb);
            let d = open_ok(&win, &file);
            d.state.borrow_mut().language_override = Some("python3".into());
            d.detect_language();
            d.set_selection(notey::core::session::Selection { start: Cursor { line: 1, column: 1 }, end: Cursor { line: 2, column: 3 } });
            // Changing the line ending marks the tab unsaved, as the status-bar menu does.
            d.state.borrow_mut().line_ending = notey::core::line_ending::LineEnding::Crlf;
            d.buffer.set_modified(true);
            quit(&win);
            drop(win);
            let win2 = env.window(&sb);
            let r = win2.current_doc().unwrap();
            assert_eq!(r.language_id().as_deref(), Some("python3"));
            assert_eq!(r.state.borrow().line_ending.label(), "CRLF");
            let sel = r.selection().expect("selection restored");
            assert_eq!((sel.start.line, sel.start.column, sel.end.line, sel.end.column), (1, 1, 2, 3));
        }),
        scenario("session_scroll_position_is_restored_once_the_view_has_a_size", |env| {
            let sb = env.sandbox();
            let body: String = (1..=600).map(|n| format!("line {n}\n")).collect();
            let file = sb.write("long.txt", &body);
            let win = env.window(&sb);
            win.window.present();
            let d = open_ok(&win, &file);
            pump_for(300);
            d.scroll_to_line_top(300);
            pump_for(300);
            let before = d.top_visible_line();
            assert!(before > 250, "scrolled to {before}");
            quit(&win);
            drop(win);
            let win2 = env.window(&sb);
            win2.window.present();
            pump_for(600);
            let top = win2.current_doc().unwrap().top_visible_line();
            assert!(top.abs_diff(before) <= 3, "restored top line {top}, expected about {before}");
        }),
        scenario("session_file_changed_while_away_keeps_my_text_and_warns", |env| {
            let sb = env.sandbox();
            let file = sb.write("shared.txt", "v1\n");
            let win = env.window(&sb);
            let d = open_ok(&win, &file);
            append(&d, "my edit\n");
            quit(&win);
            drop(win);
            sb.write("shared.txt", "v2 from another editor\n");
            let win2 = env.window(&sb);
            let r = win2.current_doc().unwrap();
            assert_eq!(r.text(), "v1\nmy edit\n", "my unsaved text wins until I choose");
            assert_eq!(info_rows(&r), 1, "a Reload / Keep Mine banner is shown");
            assert_eq!(sb.read("shared.txt"), b"v2 from another editor\n");
        }),
        scenario("session_file_deleted_while_away_keeps_my_text_and_warns", |env| {
            let sb = env.sandbox();
            let file = sb.write("gone.txt", "v1\n");
            let win = env.window(&sb);
            let d = open_ok(&win, &file);
            append(&d, "unsaved\n");
            quit(&win);
            drop(win);
            std::fs::remove_file(sb.path("gone.txt")).unwrap();
            let win2 = env.window(&sb);
            let r = win2.current_doc().unwrap();
            assert_eq!(r.text(), "v1\nunsaved\n");
            assert_eq!(info_rows(&r), 1);
        }),
        scenario("session_clean_tab_whose_file_vanished_is_kept_with_a_notice", |env| {
            let sb = env.sandbox();
            let file = sb.write("vanish.txt", "x\n");
            let win = env.window(&sb);
            open_ok(&win, &file);
            quit(&win);
            drop(win);
            std::fs::remove_file(sb.path("vanish.txt")).unwrap();
            let win2 = env.window(&sb);
            let r = win2.current_doc().unwrap();
            assert_eq!(r.name(), "vanish.txt", "the tab is not silently dropped");
            assert_eq!(info_rows(&r), 1);
        }),
        scenario("session_a_lost_buffer_file_falls_back_to_the_disk_version", |env| {
            let sb = env.sandbox();
            let file = sb.write("f.txt", "on disk\n");
            let win = env.window(&sb);
            let d = open_ok(&win, &file);
            append(&d, "typed\n");
            let id = d.id.clone();
            quit(&win);
            drop(win);
            std::fs::remove_file(sb.buffer_file(&id)).unwrap();
            let win2 = env.window(&sb);
            let r = win2.current_doc().unwrap();
            assert_eq!(r.text(), "on disk\n");
            assert_eq!(info_rows(&r), 1, "told that the unsaved text could not be recovered");
        }),
        scenario("session_one_bad_tab_does_not_block_the_others", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let a = win.current_doc().unwrap();
            append(&a, "first");
            let b = win.new_tab();
            append(&b, "second");
            let (ida, idb) = (a.id.clone(), b.id.clone());
            quit(&win);
            drop(win);
            std::fs::write(sb.buffer_file(&ida), [0xff, 0xfe, 0x00]).unwrap(); // not valid UTF-8
            let win2 = env.window(&sb);
            let texts: Vec<(String, String)> = win2.docs_in_order().iter().map(|d| (d.id.clone(), d.text())).collect();
            assert!(texts.iter().any(|(id, t)| *id == idb && t == "second"));
            assert_eq!(texts.len(), 2);
        }),
        scenario("session_with_remember_tabs_off_asks_before_losing_text", |env| {
            let sb = env.sandbox();
            let settings = Settings { remember_tabs: false, ..Default::default() };
            let win = env.window_with(&sb, "session", settings);
            let d = win.current_doc().unwrap();
            assert!(!win.needs_quit_confirmation(), "nothing to lose");
            append(&d, "unsaved");
            assert!(win.needs_quit_confirmation());
            win.shutdown(false);
            assert!(!sb.session_dir().join("session.json").exists(), "nothing is remembered");
        }),
        scenario("session_a_signal_keeps_unsaved_text_even_with_remember_tabs_off", |env| {
            let sb = env.sandbox();
            let settings = Settings { remember_tabs: false, ..Default::default() };
            let win = env.window_with(&sb, "session", settings);
            append(&win.current_doc().unwrap(), "do not lose me");
            win.shutdown(true);
            drop(win);
            let win2 = env.window(&sb);
            assert_eq!(win2.current_doc().unwrap().text(), "do not lose me");
        }),
        scenario("session_closing_the_last_tab_leaves_a_fresh_untitled_one", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = win.current_doc().unwrap();
            discard_tab(&win, &d);
            assert_eq!(win.docs.borrow().len(), 1);
            assert_eq!(win.current_doc().unwrap().name(), "Untitled");
        }),
        scenario("session_shutdown_is_idempotent_and_freezes_later_writes", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            append(&win.current_doc().unwrap(), "kept");
            win.shutdown(false);
            let manifest = std::fs::read_to_string(sb.session_dir().join("session.json")).unwrap();
            win.new_tab(); // as teardown handlers might after the flush
            win.write_manifest_now();
            win.shutdown(false);
            assert_eq!(std::fs::read_to_string(sb.session_dir().join("session.json")).unwrap(), manifest);
        }),
    ]
}

use std::rc::Rc;
