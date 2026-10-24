//! Tab context menu, app-level windows, and bracket auto-closing.
use super::dialogs::{ask, notify, Tone};
use super::doc::Doc;
use super::window::Notey;
use adw::prelude::*;
use std::rc::Rc;

impl Notey {
    fn context_doc(&self) -> Option<Rc<Doc>> {
        let page = self.context_page.borrow().clone().or_else(|| self.tab_view.selected_page())?;
        self.find_doc(&page.child())
    }

    fn add_ctx_action(self: &Rc<Self>, name: &str, f: impl Fn(&Rc<Notey>, Rc<Doc>, adw::TabPage) + 'static) {
        let a = gio::SimpleAction::new(name, None);
        let weak = Rc::downgrade(self);
        a.connect_activate(move |_, _| {
            let Some(t) = weak.upgrade() else { return };
            if let Some(doc) = t.context_doc() {
                if let Some(page) = t.page_of(&doc) {
                    f(&t, doc, page);
                }
            }
        });
        self.window.add_action(&a);
    }

    pub fn install_tab_menu(self: &Rc<Self>) {
        let menu = gio::Menu::new();
        let save = gio::Menu::new();
        save.append(Some("Save"), Some("win.tab-save"));
        save.append(Some("Save As…"), Some("win.tab-save-as"));
        menu.append_section(None, &save);
        let close = gio::Menu::new();
        close.append(Some("Close"), Some("win.tab-close"));
        close.append(Some("Close Others"), Some("win.tab-close-others"));
        close.append(Some("Close to the Right"), Some("win.tab-close-right"));
        menu.append_section(None, &close);
        let tab = gio::Menu::new();
        tab.append(Some("Pin Tab"), Some("win.tab-pin"));
        tab.append(Some("Rename…"), Some("win.tab-rename"));
        tab.append(Some("Reload from Disk"), Some("win.tab-reload"));
        menu.append_section(None, &tab);
        let file = gio::Menu::new();
        file.append(Some("Copy Path"), Some("win.tab-copy-path"));
        file.append(Some("Copy Name"), Some("win.tab-copy-name"));
        file.append(Some("Show in Files"), Some("win.tab-show"));
        menu.append_section(None, &file);
        self.tab_view.set_menu_model(Some(&menu));
        let weak = Rc::downgrade(self);
        self.tab_view.connect_setup_menu(move |_, page| {
            if let Some(t) = weak.upgrade() {
                *t.context_page.borrow_mut() = page.cloned();
            }
        });

        // These act on the tab that was right-clicked, which need not be the selected one.
        self.add_ctx_action("tab-save", |t, d, _| t.spawn_save(&d, false));
        self.add_ctx_action("tab-save-as", |t, d, _| t.spawn_save(&d, true));
        self.add_ctx_action("tab-close", |t, _, p| {
            t.tab_view.close_page(&p);
        });
        self.add_ctx_action("tab-close-others", |t, _, p| {
            for page in (0..t.tab_view.n_pages()).map(|i| t.tab_view.nth_page(i)).collect::<Vec<_>>() {
                if page != p && !page.is_pinned() {
                    t.tab_view.close_page(&page);
                }
            }
        });
        self.add_ctx_action("tab-close-right", |t, _, p| {
            let pos = t.tab_view.page_position(&p);
            for page in (pos + 1..t.tab_view.n_pages()).map(|i| t.tab_view.nth_page(i)).collect::<Vec<_>>() {
                if !page.is_pinned() {
                    t.tab_view.close_page(&page);
                }
            }
        });
        self.add_ctx_action("tab-pin", |t, _, p| {
            t.tab_view.set_page_pinned(&p, !p.is_pinned());
            t.write_manifest_now();
        });
        self.add_ctx_action("tab-reload", |t, d, _| {
            if d.path().is_some() {
                let forced = Some(d.state.borrow().encoding);
                t.spawn_reload(&d, forced);
            }
        });
        self.add_ctx_action("tab-copy-path", |t, d, _| {
            if let Some(p) = d.path() {
                t.window.clipboard().set_text(&p.to_string_lossy());
            }
        });
        self.add_ctx_action("tab-copy-name", |t, d, _| t.window.clipboard().set_text(&d.name()));
        self.add_ctx_action("tab-show", |t, d, _| {
            if let Some(p) = d.path() {
                let (w, f) = (t.window.clone(), gio::File::for_path(p));
                glib::spawn_future_local(async move {
                    let _ = gtk::FileLauncher::new(Some(&f)).open_containing_folder_future(Some(&w)).await;
                });
            }
        });
        self.add_ctx_action("tab-rename", |t, d, _| t.rename_dialog(&d));

        let customize = gio::SimpleAction::new("customize-quick", None);
        let weak = Rc::downgrade(self);
        customize.connect_activate(move |_, _| {
            if let Some(t) = weak.upgrade() {
                t.show_preferences_page(Some("quick-actions"));
            }
        });
        self.window.add_action(&customize);
        for (name, f) in [
            ("preferences", Notey::show_preferences as fn(&Rc<Notey>)),
            ("shortcuts", Notey::show_shortcuts),
            ("about", Notey::show_about),
        ] {
            let a = gio::SimpleAction::new(name, None);
            let weak = Rc::downgrade(self);
            a.connect_activate(move |_, _| {
                if let Some(t) = weak.upgrade() {
                    f(&t);
                }
            });
            self.window.add_action(&a);
        }
    }

    fn rename_dialog(self: &Rc<Self>, doc: &Rc<Doc>) {
        let Some(old) = doc.path() else {
            self.toast("Save the file first; untitled tabs have no name on disk");
            return;
        };
        let (this, doc) = (self.clone(), doc.clone());
        glib::spawn_future_local(async move {
            let dialog = adw::MessageDialog::new(Some(&this.window), Some("Rename File"), Some(&old.display().to_string()));
            let entry = gtk::Entry::builder().activates_default(true).text(doc.name()).build();
            dialog.set_extra_child(Some(&entry));
            dialog.add_response("cancel", "Cancel");
            dialog.add_response("rename", "Rename");
            dialog.set_response_appearance("rename", adw::ResponseAppearance::Suggested);
            dialog.set_default_response(Some("rename"));
            dialog.set_close_response("cancel");
            entry.select_region(0, entry.text().rfind('.').map_or(-1, |i| i as i32));
            if dialog.choose_future().await != "rename" {
                return;
            }
            let name = entry.text().to_string();
            if name.is_empty() || name.contains('/') {
                notify(&this.window, "Invalid name", "A file name cannot be empty or contain “/”.").await;
                return;
            }
            let new = old.with_file_name(&name);
            if new == old {
                return;
            }
            if new.exists() {
                let a = ask(&this.window, "Replace existing file?", &format!("{} already exists.", new.display()), &[("cancel", "Cancel", Tone::Normal), ("replace", "Replace", Tone::Destructive)], "cancel", "cancel").await;
                if a != "replace" {
                    return;
                }
            }
            match std::fs::rename(&old, &new) {
                Ok(()) => {
                    doc.set_path(Some(&new));
                    doc.state.borrow_mut().disk = crate::application::file_io::disk_state_of(&new, None).ok();
                    if doc.state.borrow().language_override.is_none() {
                        doc.detect_language();
                    }
                    this.watch(&doc);
                    this.refresh_page(&doc);
                    this.update_status();
                    this.write_manifest_now();
                }
                Err(e) => notify(&this.window, "Can’t rename", &e.to_string()).await,
            }
        });
    }

    /// Typing an opening bracket or quote adds its partner; typing the partner steps over it.
    pub fn auto_close_hook(self: &Rc<Self>, doc: &Rc<Doc>) {
        let (weak, wd) = (Rc::downgrade(self), Rc::downgrade(doc));
        let key = gtk::EventControllerKey::new();
        key.set_propagation_phase(gtk::PropagationPhase::Capture);
        key.connect_key_pressed(move |_, keyval, _, state| {
            let (Some(t), Some(d)) = (weak.upgrade(), wd.upgrade()) else { return glib::Propagation::Proceed };
            let mods = state & gtk::accelerator_get_default_mod_mask();
            let plain = mods.is_empty() || mods == gtk::gdk::ModifierType::SHIFT_MASK;
            if !t.settings.borrow().auto_close_brackets || !plain || !d.view.is_editable() {
                return glib::Propagation::Proceed;
            }
            let Some(c) = keyval.to_unicode() else { return glib::Propagation::Proceed };
            let close = match c {
                '(' => ')',
                '[' => ']',
                '{' => '}',
                '"' | '\'' | '`' => c,
                ')' | ']' | '}' => c,
                _ => return glib::Propagation::Proceed,
            };
            let buf = &d.buffer;
            let at = buf.iter_at_offset(buf.cursor_position());
            let next = at.char();
            let prev = { let mut p = at; if p.backward_char() { Some(p.char()) } else { None } };
            let is_quote = matches!(c, '"' | '\'' | '`');
            let is_closer = matches!(c, ')' | ']' | '}');
            if (is_closer || is_quote) && next == c && buf.selection_bounds().is_none() {
                let mut it = at;
                it.forward_char();
                buf.place_cursor(&it);
                return glib::Propagation::Stop;
            }
            if is_closer {
                return glib::Propagation::Proceed;
            }
            if let Some((s, e)) = buf.selection_bounds() {
                let text = buf.text(&s, &e, true);
                let (so, eo) = (s.offset(), e.offset());
                buf.begin_user_action();
                buf.delete(&mut buf.iter_at_offset(so), &mut buf.iter_at_offset(eo));
                buf.insert(&mut buf.iter_at_offset(so), &format!("{c}{text}{close}"));
                buf.end_user_action();
                buf.select_range(&buf.iter_at_offset(so + 1), &buf.iter_at_offset(eo + 1));
                return glib::Propagation::Stop;
            }
            let word_before = prev.is_some_and(|p| p.is_alphanumeric());
            let blocked = (is_quote && (word_before || next.is_alphanumeric())) || (!is_quote && next.is_alphanumeric());
            if blocked {
                return glib::Propagation::Proceed;
            }
            buf.begin_user_action();
            buf.insert_at_cursor(&format!("{c}{close}"));
            buf.end_user_action();
            let mut it = buf.iter_at_offset(buf.cursor_position());
            it.backward_char();
            buf.place_cursor(&it);
            glib::Propagation::Stop
        });
        doc.view.add_controller(key);
    }
}
