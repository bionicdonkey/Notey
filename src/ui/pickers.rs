//! Status-bar menus: language, indentation, encoding, line ending; and the encoding chooser.
use super::dialogs::{ask, Tone};
use super::doc::Doc;
use super::window::Notey;
use crate::core::encoding::{self, Encoding};
use crate::core::line_ending::LineEnding;
use crate::core::text_ops::IndentStyle;
use adw::prelude::*;
use sourceview5::prelude::*;
use std::rc::Rc;

fn menu_button_item(label: &str) -> gtk::Button {
    let b = gtk::Button::with_label(label);
    b.set_has_frame(false);
    if let Some(l) = b.child().and_downcast::<gtk::Label>() {
        l.set_xalign(0.0);
    }
    b
}

impl Notey {
    pub fn install_status_menus(self: &Rc<Self>) {
        self.build_encoding_menu();
        self.build_line_ending_menu();
        self.build_language_menu();
        self.build_indent_menu();
    }

    fn popover_box() -> (gtk::Popover, gtk::Box) {
        let b = gtk::Box::new(gtk::Orientation::Vertical, 2);
        b.set_margin_top(6);
        b.set_margin_bottom(6);
        b.set_margin_start(6);
        b.set_margin_end(6);
        let p = gtk::Popover::new();
        p.set_child(Some(&b));
        (p, b)
    }

    // ---------------------------------------------------------------- encoding

    fn build_encoding_menu(self: &Rc<Self>) {
        let (pop, bx) = Self::popover_box();
        let weak = Rc::downgrade(self);
        let item = |label: &str, f: fn(&Rc<Notey>, &Rc<Doc>)| {
            let b = menu_button_item(label);
            let (weak, pop) = (weak.clone(), pop.clone());
            b.connect_clicked(move |_| {
                pop.popdown();
                if let Some(t) = weak.upgrade() {
                    if let Some(d) = t.current_doc() {
                        f(&t, &d);
                    }
                }
            });
            b
        };
        bx.append(&item("Reopen with Encoding…", |t, d| t.choose_encoding(d, true)));
        bx.append(&item("Save with Encoding…", |t, d| t.choose_encoding(d, false)));
        bx.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        bx.append(&item("Toggle Byte Order Mark (UTF-8 BOM)", |t, d| {
            let enc = d.state.borrow().encoding;
            if enc != Encoding::Utf8 {
                t.toast("Only UTF-8 has an optional byte order mark");
                return;
            }
            let bom = !d.state.borrow().bom;
            d.state.borrow_mut().bom = bom;
            d.buffer.set_modified(true);
            t.update_status();
        }));
        bx.append(&item("Check Conversion", |t, d| t.check_conversion(d)));
        self.status.encoding.set_popover(Some(&pop));
    }

    pub fn choose_encoding(self: &Rc<Self>, doc: &Rc<Doc>, reopen: bool) {
        if reopen && doc.path().is_none() {
            self.toast("Untitled tabs are already text; use Save with Encoding instead");
            return;
        }
        let win = adw::Window::builder()
            .transient_for(&self.window)
            .modal(true)
            .default_width(360)
            .default_height(480)
            .title(if reopen { "Reopen with Encoding" } else { "Save with Encoding" })
            .build();
        let list = gtk::ListBox::new();
        list.add_css_class("boxed-list");
        list.set_selection_mode(gtk::SelectionMode::Single);
        let current = doc.state.borrow().encoding;
        let all = Encoding::all();
        for e in &all {
            let row = adw::ActionRow::builder().title(e.label()).activatable(true).build();
            if *e == current {
                row.add_suffix(&gtk::Image::from_icon_name("object-select-symbolic"));
            }
            list.append(&row);
        }
        let search = gtk::SearchEntry::new();
        let (s2, all2) = (search.clone(), all.clone());
        list.set_filter_func(move |row| {
            let q = s2.text().to_lowercase();
            q.is_empty() || all2[row.index() as usize].label().to_lowercase().contains(&q)
        });
        let l2 = list.clone();
        search.connect_search_changed(move |_| l2.invalidate_filter());
        let scroller = gtk::ScrolledWindow::builder().child(&list).vexpand(true).build();
        let body = gtk::Box::new(gtk::Orientation::Vertical, 6);
        body.set_margin_top(6);
        body.set_margin_bottom(12);
        body.set_margin_start(12);
        body.set_margin_end(12);
        body.append(&search);
        body.append(&scroller);
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&adw::HeaderBar::new());
        toolbar.set_content(Some(&body));
        win.set_content(Some(&toolbar));

        let (weak, wd, w2) = (Rc::downgrade(self), Rc::downgrade(doc), win.clone());
        list.connect_row_activated(move |_, row| {
            w2.close();
            let (Some(t), Some(d)) = (weak.upgrade(), wd.upgrade()) else { return };
            let enc = all[row.index() as usize];
            if reopen {
                t.reopen_as(&d, enc);
            } else {
                t.save_as_encoding(&d, enc);
            }
        });
        win.present();
        search.grab_focus();
    }

    fn reopen_as(self: &Rc<Self>, doc: &Rc<Doc>, enc: Encoding) {
        let (this, doc) = (self.clone(), doc.clone());
        glib::spawn_future_local(async move {
            if doc.is_dirty() {
                let a = ask(
                    &this.window,
                    "Discard your edits?",
                    "Reopening re-reads the file from disk, so unsaved changes in this tab will be lost.",
                    &[("cancel", "Cancel", Tone::Normal), ("reopen", "Reopen", Tone::Destructive)],
                    "cancel",
                    "cancel",
                )
                .await;
                if a != "reopen" {
                    return;
                }
            }
            if this.reload(&doc, Some(enc)).await {
                let mut st = doc.state.borrow_mut();
                st.guessed_encoding = false;
                drop(st);
                this.update_status();
            }
        });
    }

    fn save_as_encoding(self: &Rc<Self>, doc: &Rc<Doc>, enc: Encoding) {
        {
            let mut st = doc.state.borrow_mut();
            st.encoding = enc;
            st.bom = matches!(enc, Encoding::Utf16Le | Encoding::Utf16Be | Encoding::Utf32Le | Encoding::Utf32Be);
            st.guessed_encoding = false;
            st.had_errors = false;
        }
        doc.buffer.set_modified(true);
        let bad = encoding::check_convertible(&doc.text(), enc);
        if bad.is_empty() {
            self.toast(&format!("Will save as {}", enc.label()));
        } else {
            self.toast(&format!("{} characters can’t be represented in {}; you’ll be asked when saving", bad.len(), enc.label()));
        }
        self.update_status();
        self.write_manifest_now();
    }

    fn check_conversion(self: &Rc<Self>, doc: &Rc<Doc>) {
        let enc = doc.state.borrow().encoding;
        let bad = encoding::check_convertible(&doc.text(), enc);
        if bad.is_empty() {
            self.toast(&format!("Everything can be saved as {} without loss", enc.label()));
            return;
        }
        let first = bad[0].clone();
        let list = bad.iter().take(10).map(|u| format!("“{}” at line {}, column {}", u.ch, u.line, u.column)).collect::<Vec<_>>().join("\n");
        let (this, doc) = (self.clone(), doc.clone());
        glib::spawn_future_local(async move {
            let a = ask(
                &this.window,
                &format!("{} characters don’t fit in {}", bad.len(), enc.label()),
                &list,
                &[("close", "Close", Tone::Normal), ("go", "Go to First", Tone::Suggested)],
                "go",
                "close",
            )
            .await;
            if a == "go" {
                this.goto_line(&doc, first.line as u32, Some(first.column as u32));
            }
        });
    }

    // ------------------------------------------------------------ line endings

    fn build_line_ending_menu(self: &Rc<Self>) {
        let (pop, bx) = Self::popover_box();
        for (label, le) in [("LF (Unix, macOS)", LineEnding::Lf), ("CRLF (Windows)", LineEnding::Crlf), ("CR (classic Mac)", LineEnding::Cr)] {
            let b = menu_button_item(label);
            let (weak, pop) = (Rc::downgrade(self), pop.clone());
            b.connect_clicked(move |_| {
                pop.popdown();
                if let Some(t) = weak.upgrade() {
                    if let Some(d) = t.current_doc() {
                        {
                            let mut st = d.state.borrow_mut();
                            st.line_ending = le;
                            st.mixed_endings = false;
                        }
                        d.buffer.set_modified(true);
                        t.update_status();
                        t.write_manifest_now();
                    }
                }
            });
            bx.append(&b);
        }
        self.status.line_ending.set_popover(Some(&pop));
    }

    // ---------------------------------------------------------------- language

    fn build_language_menu(self: &Rc<Self>) {
        let manager = sourceview5::LanguageManager::default();
        let mut langs: Vec<(String, String)> = manager
            .language_ids()
            .iter()
            .filter_map(|id| manager.language(id))
            .filter(|l| !l.is_hidden())
            .map(|l| (l.name().to_string(), l.id().to_string()))
            .collect();
        langs.sort_by_key(|(n, _)| n.to_lowercase());
        let mut entries = vec![("Auto Detect".to_string(), String::new()), ("Plain Text".to_string(), "plain".to_string())];
        entries.extend(langs.into_iter().filter(|(_, id)| id != "plain" && id != "text"));

        let (pop, bx) = Self::popover_box();
        bx.set_spacing(6);
        let search = gtk::SearchEntry::new();
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::None);
        for (name, _) in &entries {
            let l = gtk::Label::new(Some(name));
            l.set_xalign(0.0);
            l.set_margin_top(4);
            l.set_margin_bottom(4);
            l.set_margin_start(6);
            list.append(&l);
        }
        let (s2, e2) = (search.clone(), entries.clone());
        list.set_filter_func(move |row| {
            let q = s2.text().to_lowercase();
            q.is_empty() || e2[row.index() as usize].0.to_lowercase().contains(&q)
        });
        let l2 = list.clone();
        search.connect_search_changed(move |_| l2.invalidate_filter());
        let scroller = gtk::ScrolledWindow::builder().child(&list).min_content_height(320).min_content_width(240).build();
        bx.append(&search);
        bx.append(&scroller);
        let (weak, pop2) = (Rc::downgrade(self), pop.clone());
        list.connect_row_activated(move |_, row| {
            pop2.popdown();
            let Some(t) = weak.upgrade() else { return };
            let Some(d) = t.current_doc() else { return };
            let id = &entries[row.index() as usize].1;
            d.state.borrow_mut().language_override = if id.is_empty() { None } else { Some(id.clone()) };
            d.detect_language();
            t.update_status();
            t.write_manifest_now();
        });
        pop.connect_show({
            let search = search.clone();
            move |_| {
                search.set_text("");
                search.grab_focus();
            }
        });
        self.status.language.set_popover(Some(&pop));
    }

    // ------------------------------------------------------------- indentation

    fn build_indent_menu(self: &Rc<Self>) {
        let (pop, bx) = Self::popover_box();
        bx.set_spacing(6);
        let spaces = gtk::CheckButton::with_label("Indent with spaces");
        let tabs = gtk::CheckButton::with_label("Indent with tabs");
        tabs.set_group(Some(&spaces));
        let width = gtk::SpinButton::with_range(1.0, 16.0, 1.0);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        row.append(&gtk::Label::new(Some("Width")));
        row.append(&width);
        let make_default = gtk::Button::with_label("Use as Default for New Tabs");
        bx.append(&spaces);
        bx.append(&tabs);
        bx.append(&row);
        bx.append(&make_default);

        let guard = Rc::new(std::cell::Cell::new(false));
        let apply = {
            let (weak, spaces, width, guard) = (Rc::downgrade(self), spaces.clone(), width.clone(), guard.clone());
            move || {
                if guard.get() {
                    return;
                }
                let Some(t) = weak.upgrade() else { return };
                let Some(d) = t.current_doc() else { return };
                let n = width.value() as usize;
                d.state.borrow_mut().indent = Some(if spaces.is_active() { IndentStyle::Spaces(n) } else { IndentStyle::Tabs });
                // Tab-stop display follows the width even when indenting with tabs.
                d.apply_settings(&t.settings.borrow());
                d.view.set_tab_width(n as u32);
                d.view.set_indent_width(n as i32);
                t.update_status();
            }
        };
        spaces.connect_toggled({
            let apply = apply.clone();
            move |_| apply()
        });
        width.connect_value_changed({
            let apply = apply.clone();
            move |_| apply()
        });
        make_default.connect_clicked({
            let (weak, spaces, width, pop) = (Rc::downgrade(self), spaces.clone(), width.clone(), pop.clone());
            move |_| {
                pop.popdown();
                if let Some(t) = weak.upgrade() {
                    {
                        let mut s = t.settings.borrow_mut();
                        s.indent_width = width.value() as u32;
                        s.insert_spaces = spaces.is_active();
                    }
                    t.apply_settings_everywhere();
                    t.toast(&format!("New tabs will indent with {} {}", width.value() as u32, if spaces.is_active() { "spaces" } else { "columns (tabs)" }));
                }
            }
        });
        let weak = Rc::downgrade(self);
        pop.connect_show(move |_| {
            let Some(t) = weak.upgrade() else { return };
            let Some(d) = t.current_doc() else { return };
            guard.set(true);
            spaces.set_active(d.view.is_insert_spaces_instead_of_tabs());
            tabs.set_active(!d.view.is_insert_spaces_instead_of_tabs());
            width.set_value(f64::from(d.view.indent_width().max(1)));
            guard.set(false);
        });
        self.status.indent.set_popover(Some(&pop));
    }
}
