//! Opening, saving, reloading and watching files.
use super::support::*;
use gtk::prelude::*;
use sourceview5::prelude::BufferExt;
use notey::application::file_io::SizeTier;
use notey::core::encoding::Encoding;
use notey::core::line_ending::LineEnding;
use notey::settings::Settings;
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/encodings").join(name)
}

pub fn scenarios() -> Vec<Scenario> {
    vec![
        scenario("files_open_reuses_the_empty_untitled_tab", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = open_ok(&win, &sb.write("a.txt", "x"));
            assert_eq!(win.docs.borrow().len(), 1);
            assert_eq!(d.name(), "a.txt");
        }),
        scenario("files_open_does_not_reuse_a_tab_with_text", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            append(&win.current_doc().unwrap(), "typed");
            open_ok(&win, &sb.write("a.txt", "x"));
            assert_eq!(win.docs.borrow().len(), 2);
        }),
        scenario("files_the_same_file_opens_in_one_tab_only", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let f = sb.write("a.txt", "x");
            let first = open_ok(&win, &f);
            win.new_tab();
            let again = open_ok(&win, &f);
            assert!(std::rc::Rc::ptr_eq(&first, &again));
            assert_eq!(win.docs.borrow().len(), 2);
            assert!(std::rc::Rc::ptr_eq(&win.current_doc().unwrap(), &first), "and it is selected");
        }),
        scenario("files_open_follows_symlinks_to_one_document", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let real = sb.write("real.txt", "x");
            let link = sb.path("link.txt");
            std::os::unix::fs::symlink(&real, &link).unwrap();
            let a = open_ok(&win, &real);
            let b = open_ok(&win, &link);
            assert!(std::rc::Rc::ptr_eq(&a, &b));
        }),
        scenario("files_every_fixture_saves_back_byte_identical", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            for name in ["utf8_plain.txt", "utf8_bom_crlf.txt", "utf16le_bom.txt", "utf16be_bom.txt", "utf32le_bom.txt", "utf16le_nobom.txt", "cr_only.txt", "no_trailing_newline.txt", "cp1251.txt", "sjis.txt", "gbk.txt", "latin1_cafe.txt", "empty.txt"] {
                let copy = sb.write(name, std::fs::read(fixture(name)).unwrap());
                let before = std::fs::read(&copy).unwrap();
                let d = open_ok(&win, &copy);
                assert!(save(&win, &d), "{name}: save failed");
                assert_eq!(std::fs::read(&copy).unwrap(), before, "{name}: bytes changed on an unmodified save");
            }
        }),
        scenario("files_changing_line_ending_and_encoding_applies_on_save", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = open_ok(&win, &sb.write("t.txt", "one\ntwo\n"));
            {
                let mut st = d.state.borrow_mut();
                st.line_ending = LineEnding::Crlf;
                st.encoding = Encoding::Utf16Le;
                st.bom = true;
            }
            d.buffer.set_modified(true);
            assert!(save(&win, &d));
            let expected = notey::core::encoding::encode("one\ntwo\n", Encoding::Utf16Le, true, LineEnding::Crlf).bytes;
            assert_eq!(sb.read("t.txt"), expected);
            // And it reads back as what it now is.
            let again = notey::application::file_io::read_file(&sb.path("t.txt"), None).unwrap();
            assert_eq!((again.decoded.encoding, again.decoded.line_ending, again.decoded.text.as_str()), (Encoding::Utf16Le, LineEnding::Crlf, "one\ntwo\n"));
        }),
        scenario("files_reopen_with_another_encoding_changes_the_text_only", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let copy = sb.write("ru.txt", std::fs::read(fixture("cp1251.txt")).unwrap());
            let d = open_ok(&win, &copy);
            assert!(d.text().contains("Привет"), "detected correctly");
            assert!(d.state.borrow().guessed_encoding);
            let (w, dd) = (win.clone(), d.clone());
            block_on(async move { w.reload(&dd, Encoding::from_name("windows-1252")).await });
            assert!(!d.text().contains("Привет"), "forced the wrong encoding");
            let (w, dd) = (win.clone(), d.clone());
            block_on(async move { w.reload(&dd, Encoding::from_name("windows-1251")).await });
            assert!(d.text().contains("Привет"));
            assert_eq!(std::fs::read(&copy).unwrap(), std::fs::read(fixture("cp1251.txt")).unwrap(), "the file was not touched");
        }),
        scenario("files_a_guessed_encoding_is_flagged_and_explained", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = open_ok(&win, &sb.write("ru.txt", std::fs::read(fixture("cp1251.txt")).unwrap()));
            assert_eq!(info_rows(&d), 1, "a banner offers Reopen With Encoding");
            let plain = open_ok(&win, &sb.write("p.txt", "plain\n"));
            assert_eq!(info_rows(&plain), 0);
        }),
        scenario("files_a_new_path_opens_empty_and_saves_to_disk", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let target = sb.path("notes/new.md");
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            let d = open_ok(&win, &target);
            assert_eq!((d.text().as_str(), d.is_dirty()), ("", false));
            append(&d, "# Title\n");
            assert!(save(&win, &d));
            assert_eq!(sb.read("notes/new.md"), b"# Title\n");
            assert_eq!(d.language_id().as_deref(), Some("markdown"));
        }),
        scenario("files_external_change_reloads_a_clean_tab", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = open_ok(&win, &sb.write("w.txt", "old\n"));
            sb.write("w.txt", "new content from elsewhere\n");
            let (w, dd) = (win.clone(), d.clone());
            block_on(async move { w.check_external(&dd).await });
            assert_eq!(d.text(), "new content from elsewhere\n");
            assert!(!d.is_dirty());
        }),
        scenario("files_external_change_on_an_edited_tab_asks_and_keeps_my_text", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = open_ok(&win, &sb.write("w.txt", "old\n"));
            append(&d, "mine\n");
            sb.write("w.txt", "theirs\n");
            let (w, dd) = (win.clone(), d.clone());
            block_on(async move { w.check_external(&dd).await });
            assert_eq!(d.text(), "old\nmine\n");
            assert_eq!(info_rows(&d), 1);
            // A second notification does not stack another banner.
            let (w, dd) = (win.clone(), d.clone());
            block_on(async move { w.check_external(&dd).await });
            assert_eq!(info_rows(&d), 1);
        }),
        scenario("files_external_delete_warns_without_closing_the_tab", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = open_ok(&win, &sb.write("w.txt", "text\n"));
            std::fs::remove_file(sb.path("w.txt")).unwrap();
            let (w, dd) = (win.clone(), d.clone());
            block_on(async move { w.check_external(&dd).await });
            assert_eq!(info_rows(&d), 1);
            assert_eq!(d.text(), "text\n");
        }),
        scenario("files_touching_a_file_without_changing_it_is_not_a_change", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let f = sb.write("w.txt", "same\n");
            let d = open_ok(&win, &f);
            append(&d, "mine\n");
            std::fs::write(&f, "same\n").unwrap(); // new mtime, identical bytes
            let (w, dd) = (win.clone(), d.clone());
            block_on(async move { w.check_external(&dd).await });
            assert_eq!(info_rows(&d), 0);
        }),
        scenario("files_the_file_monitor_notices_a_change_by_itself", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = open_ok(&win, &sb.write("live.txt", "before\n"));
            pump_for(200);
            sb.write("live.txt", "after, written by another program\n");
            pump_until("the tab to reload", || d.text().starts_with("after"));
        }),
        scenario("files_our_own_save_is_not_reported_as_an_external_change", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = open_ok(&win, &sb.write("own.txt", "a\n"));
            append(&d, "b\n");
            assert!(save(&win, &d));
            pump_for(700);
            assert_eq!(info_rows(&d), 0);
            assert_eq!(d.text(), "a\nb\n");
        }),
        scenario("files_a_large_file_turns_highlighting_and_minimap_off", |env| {
            let sb = env.sandbox();
            let settings = Settings { show_minimap: true, ..Default::default() };
            let win = env.window_with(&sb, "session", settings);
            let big = "fn x() { let a = 1; }\n".repeat(6 * 1024 * 1024 / 22);
            let d = open_ok(&win, &sb.write("big.rs", big));
            assert_eq!(d.state.borrow().tier, SizeTier::Large);
            assert!(!d.buffer.is_highlight_syntax());
            assert!(!d.map.get_visible());
            assert!(info_rows(&d) >= 1, "told why highlighting is off");
        }),
        scenario("files_a_read_only_file_opens_but_cannot_be_edited", |env| {
            use std::os::unix::fs::PermissionsExt;
            let sb = env.sandbox();
            let win = env.window(&sb);
            let f = sb.write("ro.txt", "fixed\n");
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o444)).unwrap();
            let d = open_ok(&win, &f);
            assert!(d.state.borrow().read_only);
            assert!(!d.view.is_editable());
            assert!(win.status.note.label().contains("Read-only"));
        }),
        scenario("files_recent_files_are_newest_first_without_duplicates", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let (a, b) = (sb.write("a.txt", "a"), sb.write("b.txt", "b"));
            open_ok(&win, &a);
            open_ok(&win, &b);
            win.new_tab();
            open_ok(&win, &a);
            let recent = win.settings.borrow().recent_files.clone();
            assert_eq!(recent.len(), 2);
            assert!(recent[0].ends_with("a.txt") && recent[1].ends_with("b.txt"));
            assert!(win.recent_menu.n_items() > 0);
        }),
        scenario("files_recent_list_respects_its_size_limit", |env| {
            let sb = env.sandbox();
            let settings = Settings { recent_size: 3, ..Default::default() };
            let win = env.window_with(&sb, "session", settings);
            for i in 0..6 {
                open_ok(&win, &sb.write(&format!("f{i}.txt"), "x"));
            }
            assert_eq!(win.settings.borrow().recent_files.len(), 3);
        }),
        scenario("files_tabs_with_the_same_name_show_their_folder", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let a = open_ok(&win, &sb.write("alpha/main.rs", "a"));
            let b = open_ok(&win, &sb.write("beta/main.rs", "b"));
            assert!(page_title(&win, &a).contains("alpha"), "{}", page_title(&win, &a));
            assert!(page_title(&win, &b).contains("beta"));
            let solo = open_ok(&win, &sb.write("only.txt", "c"));
            assert_eq!(page_title(&win, &solo), "only.txt");
        }),
        scenario("files_the_tab_title_marks_unsaved_changes", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = open_ok(&win, &sb.write("t.txt", "x"));
            assert_eq!(page_title(&win, &d), "t.txt");
            append(&d, "y");
            assert_eq!(page_title(&win, &d), "• t.txt");
            assert!(save(&win, &d));
            assert_eq!(page_title(&win, &d), "t.txt");
        }),
        scenario("files_ensure_final_newline_applies_on_save_only_when_enabled", |env| {
            let sb = env.sandbox();
            let on = Settings { ensure_final_newline: true, ..Default::default() };
            let win = env.window_with(&sb, "session", on);
            let d = open_ok(&win, &sb.write("n.txt", "no newline"));
            assert!(save(&win, &d));
            assert_eq!(sb.read("n.txt"), b"no newline\n");
            let off = env.window_with(&sb, "session2", Settings::default());
            let d2 = open_ok(&off, &sb.write("m.txt", "no newline"));
            assert!(save(&off, &d2));
            assert_eq!(sb.read("m.txt"), b"no newline");
        }),
        scenario("files_save_all_writes_every_edited_file_tab", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let a = open_ok(&win, &sb.write("a.txt", "1"));
            let b = open_ok(&win, &sb.write("b.txt", "2"));
            append(&a, "a");
            append(&b, "b");
            let w = win.clone();
            block_on(async move { w.save_all().await });
            assert_eq!((sb.read("a.txt"), sb.read("b.txt")), (b"1a".to_vec(), b"2b".to_vec()));
            assert!(!a.is_dirty() && !b.is_dirty());
        }),
        scenario("files_several_files_open_in_the_order_given", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            // Different sizes so a concurrent open would finish out of order.
            let big = sb.write("c_big.txt", "x\n".repeat(400_000));
            let a = sb.write("a.txt", "a");
            let b = sb.write("b.txt", "b");
            win.open_in_background(vec![(big, None), (a, Some(1)), (b, None)]);
            pump_until("all three files to open", || win.docs.borrow().len() == 3);
            let names: Vec<String> = win.docs_in_order().iter().map(|d| d.name()).collect();
            assert_eq!(names, ["c_big.txt", "a.txt", "b.txt"]);
        }),
        scenario("files_dropped_files_open_in_tabs", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let f = sb.write("dropped.txt", "from the file manager");
            win.open_dropped(&[gio::File::for_path(&f)]);
            pump_until("the dropped file to open", || win.docs.borrow().iter().any(|d| d.name() == "dropped.txt"));
        }),
        scenario("files_renaming_moves_the_file_and_keeps_the_tab", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = open_ok(&win, &sb.write("old.txt", "x"));
            // The dialog itself is interactive; this is the same sequence it runs.
            let new = sb.path("renamed.md");
            std::fs::rename(sb.path("old.txt"), &new).unwrap();
            d.set_path(Some(&new));
            d.state.borrow_mut().disk = notey::application::file_io::disk_state_of(&new, None).ok();
            d.detect_language();
            win.watch(&d);
            win.refresh_page(&d);
            assert_eq!(page_title(&win, &d), "renamed.md");
            assert_eq!(d.language_id().as_deref(), Some("markdown"));
            pump_for(400);
            assert_eq!(info_rows(&d), 0);
        }),
    ]
}
