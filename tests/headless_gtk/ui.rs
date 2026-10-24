//! Chrome: status bar, settings applied live, quick bar, preferences, menus and shortcuts.
use super::support::*;
use adw::prelude::*;
use notey::settings::{Settings, ThemeChoice};
use notey::ui::menus::{main_menu_model, QUICK_ACTIONS, SEPARATOR};
use notey::ui::SHORTCUTS;
use sourceview5::prelude::{BufferExt, ViewExt};
use std::collections::HashSet;

fn action_names(model: &gio::MenuModel, out: &mut Vec<String>) {
    for i in 0..model.n_items() {
        if let Some(a) = model.item_attribute_value(i, "action", Some(glib::VariantTy::STRING)).and_then(|v| v.get::<String>()) {
            out.push(a);
        }
        for link in ["section", "submenu"] {
            if let Some(sub) = model.item_link(i, link) {
                action_names(&sub, out);
            }
        }
    }
}

fn mb(b: &gtk::MenuButton) -> String {
    b.label().map(|l| l.to_string()).unwrap_or_default()
}

fn kids(w: &impl IsA<gtk::Widget>) -> usize {
    let mut n = 0;
    let mut c = w.first_child();
    while let Some(x) = c {
        n += 1;
        c = x.next_sibling();
    }
    n
}

pub fn scenarios() -> Vec<Scenario> {
    vec![
        scenario("ui_status_bar_reports_position_selection_and_format", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = open_ok(&win, &sb.write("s.rs", "fn main() {\n    let x = 1;\n}\n"));
            win.goto_line(&d, 2, Some(5));
            assert_eq!(win.status.position.label(), "Ln 2, Col 5");
            assert_eq!(mb(&win.status.language), "Rust");
            assert_eq!(mb(&win.status.encoding), "UTF-8");
            assert_eq!(mb(&win.status.line_ending), "LF");
            assert!(mb(&win.status.indent).starts_with("Spaces:"));
            d.buffer.select_range(&d.buffer.iter_at_offset(0), &d.buffer.iter_at_offset(7));
            assert!(win.status.position.label().contains("7 selected"), "{}", win.status.position.label());
            select_all(&d);
            assert!(win.status.position.label().contains("lines selected"));
        }),
        scenario("ui_status_bar_shows_bom_and_flags_guessed_encodings", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let fx = |n: &str| std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/encodings").join(n)).unwrap();
            open_ok(&win, &sb.write("bom.txt", fx("utf8_bom_crlf.txt")));
            assert_eq!(mb(&win.status.encoding), "UTF-8 BOM");
            assert_eq!(mb(&win.status.line_ending), "CRLF");
            assert!(!win.status.encoding.has_css_class("guess"));
            open_ok(&win, &sb.write("ru.txt", fx("cp1251.txt")));
            assert_eq!(mb(&win.status.encoding), "windows-1251");
            assert!(win.status.encoding.has_css_class("guess"));
            open_ok(&win, &sb.write("mixed.txt", fx("mixed_endings.txt")));
            assert_eq!(mb(&win.status.line_ending), "Mixed");
        }),
        scenario("ui_language_is_detected_overridden_and_restored_to_auto", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let py = open_ok(&win, &sb.write("script", "#!/usr/bin/env python3\nprint(1)\n"));
            assert_eq!(py.language_id().as_deref(), Some("python3"), "shebang");
            let mk = open_ok(&win, &sb.write("Makefile", "all:\n\techo hi\n"));
            assert_eq!(mk.language_id().as_deref(), Some("makefile"), "file name");
            let md = open_ok(&win, &sb.write("README.md", "# hi\n"));
            assert_eq!(md.language_id().as_deref(), Some("markdown"));
            md.state.borrow_mut().language_override = Some("rust".into());
            md.detect_language();
            assert_eq!(md.language_id().as_deref(), Some("rust"));
            md.state.borrow_mut().language_override = None;
            md.detect_language();
            assert_eq!(md.language_id().as_deref(), Some("markdown"));
            let plain = open_ok(&win, &sb.write("notes.txt", "just words\n"));
            assert_eq!(plain.language_name(), "Plain Text");
        }),
        scenario("ui_every_shortcut_names_a_real_action_and_no_key_is_used_twice", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let mut seen = HashSet::new();
            for (_, title, action, keys) in SHORTCUTS {
                let name = action.strip_prefix("win.").unwrap_or_else(|| panic!("{title}: {action} is not a window action"));
                assert!(gio::prelude::ActionMapExt::lookup_action(&win.window, name).is_some(), "{title}: no action {name}");
                for k in *keys {
                    let (key, mods) = gtk::accelerator_parse(*k).unwrap_or_else(|| panic!("{title}: bad accelerator {k}"));
                    let id = gtk::accelerator_name(key, mods).to_string();
                    assert!(seen.insert(id.clone()), "{k} ({id}) is bound twice");
                }
            }
            for n in 1..=9 {
                assert!(seen.insert(format!("<Alt>{n}")), "Alt+{n} clashes with a shortcut");
            }
            assert!(gio::prelude::ActionMapExt::lookup_action(&win.window, "goto-tab").is_some());
        }),
        scenario("ui_menus_and_quick_actions_only_reference_real_actions", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let recent = gio::Menu::new();
            let mut names = Vec::new();
            action_names(main_menu_model(&recent).upcast_ref(), &mut names);
            assert!(names.len() > 40, "the main menu is populated ({} items)", names.len());
            for qa in QUICK_ACTIONS {
                names.push(format!("win.{}", qa.action));
            }
            for n in names {
                let short = n.strip_prefix("win.").unwrap_or_else(|| panic!("{n} is not a window action"));
                assert!(gio::prelude::ActionMapExt::lookup_action(&win.window, short).is_some(), "menu or quick bar refers to a missing action: {n}");
            }
        }),
        scenario("ui_tab_view_leaves_document_navigation_keys_to_the_editor", |env| {
            use adw::TabViewShortcuts as K;
            let sb = env.sandbox();
            let win = env.window(&sb);
            let k = win.tab_view.shortcuts();
            assert!(!k.intersects(K::CONTROL_HOME | K::CONTROL_END | K::CONTROL_SHIFT_HOME | K::CONTROL_SHIFT_END));
            let ctrls = win.window.observe_controllers();
            let phase = (0..ctrls.n_items())
                .filter_map(|i| ctrls.item(i).and_downcast::<gtk::EventController>())
                .find(|c| c.name().as_deref() == Some("gtk-application-shortcuts"))
                .map(|c| c.propagation_phase());
            assert_eq!(phase, Some(gtk::PropagationPhase::Capture), "app shortcuts run before the text view's own");
        }),
        scenario("ui_settings_apply_to_every_open_tab_immediately", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let a = win.current_doc().unwrap();
            let b = win.new_tab();
            {
                let mut s = win.settings.borrow_mut();
                s.word_wrap = true;
                s.line_numbers = false;
                s.show_minimap = true;
                s.highlight_current_line = false;
                s.right_margin = 80;
                s.show_whitespace = true;
            }
            win.apply_settings_everywhere();
            for d in [&a, &b] {
                assert_eq!(d.view.wrap_mode(), gtk::WrapMode::WordChar);
                assert!(!d.view.shows_line_numbers());
                assert!(d.map.get_visible());
                assert!(!d.view.is_highlight_current_line());
                assert!(d.view.shows_right_margin() && d.view.right_margin_position() == 80);
                assert!(d.view.space_drawer().enables_matrix());
            }
            win.settings.borrow_mut().word_wrap = false;
            win.apply_settings_everywhere();
            assert_eq!(a.view.wrap_mode(), gtk::WrapMode::None);
        }),
        scenario("ui_zoom_is_bounded_and_resets", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            act(&win, "zoom-in");
            act(&win, "zoom-in");
            assert_eq!(win.settings.borrow().zoom_percent, 120);
            assert_eq!(win.status.zoom.label().unwrap().to_string(), "120%");
            for _ in 0..60 {
                act(&win, "zoom-out");
            }
            assert_eq!(win.settings.borrow().zoom_percent, 50);
            for _ in 0..60 {
                act(&win, "zoom-in");
            }
            assert_eq!(win.settings.borrow().zoom_percent, 400);
            act(&win, "zoom-reset");
            assert_eq!(win.settings.borrow().zoom_percent, 100);
        }),
        scenario("ui_view_toggle_actions_update_settings_and_state", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            act(&win, "word-wrap");
            assert!(win.settings.borrow().word_wrap);
            let state = gio::prelude::ActionGroupExt::action_state(&win.window, "word-wrap").and_then(|v| v.get::<bool>());
            assert_eq!(state, Some(true));
            act(&win, "word-wrap");
            assert!(!win.settings.borrow().word_wrap);
            act(&win, "line-numbers");
            assert!(!win.settings.borrow().line_numbers);
        }),
        scenario("ui_the_colour_scheme_follows_the_theme_and_the_choice", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = win.current_doc().unwrap();
            win.settings.borrow_mut().theme = ThemeChoice::Dark;
            win.apply_settings_everywhere();
            assert_eq!(adw::StyleManager::default().color_scheme(), adw::ColorScheme::ForceDark);
            win.settings.borrow_mut().style_scheme = Some("classic".into());
            win.apply_settings_everywhere();
            assert_eq!(d.buffer.style_scheme().map(|s| s.id().to_string()).as_deref(), Some("classic"));
            win.settings.borrow_mut().theme = ThemeChoice::System;
            win.settings.borrow_mut().style_scheme = None;
            win.apply_settings_everywhere();
            assert_eq!(adw::StyleManager::default().color_scheme(), adw::ColorScheme::Default);
        }),
        scenario("ui_quick_bar_is_customisable_groupable_and_hideable", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            assert!(win.quickbar.get_visible());
            let row = |win: &notey::ui::window::NoteyWindow| win.quickbar.child().and_downcast::<gtk::Viewport>().unwrap().child().unwrap();
            let default_buttons = kids(&row(&win));
            assert!(default_buttons > 11, "default set with dividers: {default_buttons}");
            let ids = ["|", "cut", "|", "|", "copy", "paste", "|"].map(String::from).to_vec();
            win.settings.borrow_mut().quick_actions = ids;
            win.apply_settings_everywhere();
            assert_eq!(kids(&row(&win)), 4, "cut, divider, copy, paste: no leading, trailing or doubled dividers");
            win.settings.borrow_mut().quick_actions = vec!["bogus".into(), "cut".into()];
            win.apply_settings_everywhere();
            assert_eq!(kids(&row(&win)), 1, "unknown ids are ignored");
            win.settings.borrow_mut().show_toolbar = false;
            win.apply_settings_everywhere();
            assert!(!win.quickbar.get_visible());
            assert!(QUICK_ACTIONS.iter().all(|a| a.id != SEPARATOR));
        }),
        scenario("ui_quick_bar_buttons_run_their_action", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = open_ok(&win, &sb.write("q.py", "a\nb"));
            select_all(&d);
            let row = win.quickbar.child().and_downcast::<gtk::Viewport>().unwrap().child().unwrap();
            let mut c = row.first_child();
            let mut found = None;
            while let Some(w) = c {
                if let Some(b) = w.downcast_ref::<gtk::Button>() {
                    if b.tooltip_text().as_deref() == Some("Comment lines") {
                        found = Some(b.clone());
                    }
                }
                c = w.next_sibling();
            }
            found.expect("the Comment button is on the default bar").emit_clicked();
            pump_for(30);
            assert_eq!(d.text(), "# a\n# b");
        }),
        scenario("ui_preferences_is_one_tab_that_is_never_a_document", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let before = win.tab_view.n_pages();
            win.show_preferences_page(Some("quick-actions"));
            win.show_preferences();
            act(&win, "preferences");
            pump_for(50);
            assert_eq!(win.tab_view.n_pages(), before + 1, "later requests select the existing tab");
            assert!(win.current_doc().is_none());
            assert_eq!(win.title.title(), "Preferences");
            assert_eq!(win.build_session().tabs.len(), win.docs.borrow().len(), "not saved in the session");
            // Editing commands do nothing while it is selected.
            act(&win, "save");
            act(&win, "toggle-comment");
            let prefs = win.tab_view.selected_page().unwrap();
            win.tab_view.close_page(&prefs);
            pump_for(50);
            assert_eq!(win.tab_view.n_pages(), before);
            assert!(win.current_doc().is_some());
            win.show_preferences();
            assert_eq!(win.tab_view.n_pages(), before + 1, "it can be opened again after closing");
        }),
        scenario("ui_preferences_changes_apply_to_the_editor", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = win.current_doc().unwrap();
            win.show_preferences();
            pump_for(50);
            // Find the "Word wrap" check on the Editor page and click it, as a user would.
            let (page, _) = win.prefs_page.borrow().clone().unwrap();
            fn find_check(w: &gtk::Widget, label: &str) -> Option<gtk::CheckButton> {
                if let Some(c) = w.downcast_ref::<gtk::CheckButton>() {
                    if c.label().as_deref() == Some(label) {
                        return Some(c.clone());
                    }
                }
                let mut c = w.first_child();
                while let Some(x) = c {
                    if let Some(f) = find_check(&x, label) {
                        return Some(f);
                    }
                    c = x.next_sibling();
                }
                None
            }
            let check = find_check(&page, "Word wrap").expect("Word wrap check button");
            assert!(!check.is_active());
            check.set_active(true);
            assert!(win.settings.borrow().word_wrap);
            assert_eq!(d.view.wrap_mode(), gtk::WrapMode::WordChar);
            let spin = {
                fn find_spin(w: &gtk::Widget) -> Option<gtk::SpinButton> {
                    if let Some(s) = w.downcast_ref::<gtk::SpinButton>() {
                        if s.adjustment().upper() == 16.0 {
                            return Some(s.clone());
                        }
                    }
                    let mut c = w.first_child();
                    while let Some(x) = c {
                        if let Some(f) = find_spin(&x) {
                            return Some(f);
                        }
                        c = x.next_sibling();
                    }
                    None
                }
                find_spin(&page).expect("indent width spin button (1 to 16)")
            };
            assert_eq!(spin.value(), 4.0, "default indent width");
            spin.set_value(2.0);
            assert_eq!(win.settings.borrow().indent_width, 2);
            assert_eq!(d.view.indent_width(), 2);
        }),
        scenario("ui_pinning_and_close_others_leave_pinned_tabs_alone", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let a = open_ok(&win, &sb.write("a.txt", "a"));
            let b = open_ok(&win, &sb.write("b.txt", "b"));
            let c = open_ok(&win, &sb.write("c.txt", "c"));
            *win.context_page.borrow_mut() = win.page_of(&a);
            act(&win, "tab-pin");
            assert!(win.page_of(&a).unwrap().is_pinned());
            *win.context_page.borrow_mut() = win.page_of(&c);
            act(&win, "tab-close-others");
            let names: Vec<String> = win.docs_in_order().iter().map(|d| d.name()).collect();
            assert_eq!(names, vec!["a.txt".to_string(), "c.txt".to_string()], "b closed, pinned a kept");
            let _ = b;
            *win.context_page.borrow_mut() = win.page_of(&a);
            act(&win, "tab-close-right");
            assert_eq!(win.docs.borrow().len(), 1);
        }),
        scenario("ui_tab_context_menu_save_writes_the_tab_that_was_clicked", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let a = open_ok(&win, &sb.write("a.txt", "a"));
            let b = open_ok(&win, &sb.write("b.txt", "b"));
            append(&a, "1");
            append(&b, "2");
            // `b` is selected; the user right-clicks `a`'s tab header and chooses Save.
            assert!(std::rc::Rc::ptr_eq(&win.current_doc().unwrap(), &b));
            *win.context_page.borrow_mut() = win.page_of(&a);
            act(&win, "tab-save");
            pump_until("a.txt to be written", || sb.read("a.txt") == b"a1");
            assert_eq!(sb.read("b.txt"), b"b", "the selected tab was not saved");
            assert!(!a.is_dirty() && b.is_dirty());
        }),
        scenario("ui_the_tab_context_menu_offers_save_and_save_as", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let model = win.tab_view.menu_model().expect("tab menu model");
            let mut names = Vec::new();
            action_names(&model, &mut names);
            assert_eq!(&names[..2], ["win.tab-save", "win.tab-save-as"], "Save is at the top: {names:?}");
            for n in names {
                let short = n.strip_prefix("win.").unwrap();
                assert!(gio::prelude::ActionMapExt::lookup_action(&win.window, short).is_some(), "missing action {n}");
            }
        }),
        scenario("ui_window_title_and_subtitle_follow_the_selected_tab", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let a = open_ok(&win, &sb.write("sub/a.txt", "a"));
            let u = win.new_tab();
            assert_eq!((win.title.title().as_str(), win.title.subtitle().as_str()), ("Untitled", "Unsaved"));
            win.tab_view.set_selected_page(&win.page_of(&a).unwrap());
            assert_eq!(win.title.title(), "a.txt");
            assert!(win.title.subtitle().ends_with("/sub"));
            append(&a, "x");
            assert_eq!(win.title.title(), "• a.txt");
            let _ = u;
        }),
        scenario("ui_tab_navigation_actions_cycle_and_jump", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let docs: Vec<_> = ["a", "b", "c"].iter().map(|n| open_ok(&win, &sb.write(&format!("{n}.txt"), *n))).collect();
            win.tab_view.set_selected_page(&win.page_of(&docs[0]).unwrap());
            act(&win, "next-tab");
            assert_eq!(win.current_doc().unwrap().name(), "b.txt");
            act(&win, "prev-tab");
            assert_eq!(win.current_doc().unwrap().name(), "a.txt");
            gio::prelude::ActionGroupExt::activate_action(&win.window, "goto-tab", Some(&3i32.to_variant()));
            assert_eq!(win.current_doc().unwrap().name(), "c.txt");
            gio::prelude::ActionGroupExt::activate_action(&win.window, "goto-tab", Some(&9i32.to_variant()));
            assert_eq!(win.current_doc().unwrap().name(), "c.txt", "Alt+9 is the last tab");
        }),
        scenario("ui_default_settings_match_the_documented_defaults", |_| {
            let s = Settings::default();
            assert_eq!(s.indent_width, 4);
            assert!(s.insert_spaces && s.auto_indent && s.line_numbers && s.remember_tabs);
            assert!(!s.word_wrap && !s.show_minimap && !s.auto_close_brackets);
            assert_eq!(s.default_encoding, "UTF-8");
            assert_eq!(s.recent_size, 15);
            assert!(s.show_toolbar);
            assert_eq!(s.quick_actions.iter().filter(|a| *a == "|").count(), 3);
        }),
    ]
}
