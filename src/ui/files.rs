//! Opening, saving, reloading and watching files; banners; actions; the status bar.
use super::dialogs::{ask, notify, Tone};
use super::doc::Doc;
use super::window::Notey;
use crate::application::file_io::{self, LoadedFile, SizeLimits, SizeTier};
use crate::core::encoding::{self, Confidence, Encoding};
use crate::core::session::{reconcile, Reconcile};
use crate::core::text_ops::detect_indent;
use adw::prelude::*;
use sourceview5::prelude::*;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

type InfoAction = (String, Box<dyn Fn(&Rc<Notey>, &Rc<Doc>)>);

impl Notey {
    // ------------------------------------------------------------ info banners

    pub fn show_info(self: &Rc<Self>, doc: &Rc<Doc>, text: &str, warn: bool, actions: Vec<InfoAction>) -> gtk::Box {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.add_css_class("notey-infobar");
        if warn {
            row.add_css_class("warning");
        }
        let label = gtk::Label::new(Some(text));
        label.set_wrap(true);
        label.set_xalign(0.0);
        label.set_hexpand(true);
        row.append(&label);
        for (name, f) in actions {
            let b = gtk::Button::with_label(&name);
            let (weak, wd, r) = (Rc::downgrade(self), Rc::downgrade(doc), row.clone());
            b.connect_clicked(move |_| {
                if let (Some(t), Some(d)) = (weak.upgrade(), wd.upgrade()) {
                    t.dismiss_info(&d, &r);
                    f(&t, &d);
                }
            });
            row.append(&b);
        }
        let close = gtk::Button::from_icon_name("window-close-symbolic");
        close.set_has_frame(false);
        close.set_tooltip_text(Some("Dismiss"));
        let (weak, wd, r) = (Rc::downgrade(self), Rc::downgrade(doc), row.clone());
        close.connect_clicked(move |_| {
            if let (Some(t), Some(d)) = (weak.upgrade(), wd.upgrade()) {
                t.dismiss_info(&d, &r);
            }
        });
        row.append(&close);
        doc.info.append(&row);
        doc.info.set_visible(true);
        row
    }

    fn dismiss_info(&self, doc: &Doc, row: &gtk::Box) {
        if row.parent().is_some() {
            doc.info.remove(row);
        }
        doc.info.set_visible(doc.info.first_child().is_some());
        doc.external_pending.set(false);
    }

    pub fn show_notice(self: &Rc<Self>, doc: &Rc<Doc>, text: &str) {
        self.show_info(doc, text, false, Vec::new());
    }

    pub fn show_external_change(self: &Rc<Self>, doc: &Rc<Doc>, deleted: bool) {
        if doc.external_pending.replace(true) {
            return;
        }
        if deleted {
            let actions: Vec<InfoAction> = vec![
                ("Save As…".into(), Box::new(|t, d| t.spawn_save(d, true))),
                ("Keep".into(), Box::new(|_, d| d.state.borrow_mut().disk = None)),
            ];
            self.show_info(doc, "This file was deleted or moved on disk.", true, actions);
        } else {
            let actions: Vec<InfoAction> = vec![
                ("Reload".into(), Box::new(|t, d| t.spawn_reload(d, None))),
                (
                    "Keep Mine".into(),
                    Box::new(|_, d| {
                        if let Some(p) = d.path() {
                            d.state.borrow_mut().disk = file_io::disk_state_of(&p, None).ok();
                        }
                    }),
                ),
            ];
            self.show_info(doc, "This file changed on disk. Reload it, or keep your version?", true, actions);
        }
    }

    // ----------------------------------------------------------------- loading

    pub fn size_limits(&self) -> SizeLimits {
        SizeLimits::default()
    }

    /// Puts freshly read file content into a document.
    pub fn apply_loaded(&self, doc: &Rc<Doc>, loaded: LoadedFile) {
        let d = loaded.decoded;
        doc.set_text(&d.text, false);
        {
            let mut st = doc.state.borrow_mut();
            st.encoding = d.encoding;
            st.bom = d.bom;
            st.line_ending = d.line_ending;
            st.mixed_endings = d.mixed_endings;
            st.guessed_encoding = d.confidence == Confidence::Guessed;
            st.had_errors = d.had_errors;
            st.tier = self.size_limits().classify(loaded.disk.size);
            st.read_only = loaded.read_only;
            st.indent = detect_indent(&d.text);
            st.disk = Some(loaded.disk);
        }
        doc.view.set_editable(!doc.state.borrow().read_only);
        doc.apply_settings(&self.settings.borrow());
        doc.detect_language();
    }

    /// Files dropped on the window (from a file manager).
    pub fn open_dropped(self: &Rc<Self>, files: &[gio::File]) {
        let paths: Vec<PathBuf> = files.iter().filter_map(|f| f.path()).collect();
        self.open_in_background(paths, None);
    }

    pub fn open_in_background(self: &Rc<Self>, paths: Vec<PathBuf>, line: Option<u32>) {
        let this = self.clone();
        glib::spawn_future_local(async move {
            for p in paths {
                this.open_path(p, line).await;
            }
            this.window.present();
        });
    }

    pub async fn open_path(self: &Rc<Self>, path: PathBuf, line: Option<u32>) -> Option<Rc<Doc>> {
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        let existing = self.docs.borrow().iter().find(|d| d.path().as_deref() == Some(path.as_path())).cloned();
        if let Some(doc) = existing {
            if let Some(page) = self.page_of(&doc) {
                self.tab_view.set_selected_page(&page);
            }
            if let Some(l) = line {
                self.goto_line(&doc, l, None);
            }
            self.note_recent(&path);
            return Some(doc);
        }
        let meta = std::fs::metadata(&path);
        let mut tier = SizeTier::Normal;
        match &meta {
            Ok(m) if m.is_dir() => {
                notify(&self.window, "Can’t open a folder", &path.display().to_string()).await;
                return None;
            }
            Ok(m) => {
                tier = self.size_limits().classify(m.len());
                match tier {
                    SizeTier::Refuse => {
                        notify(
                            &self.window,
                            "File is too large",
                            &format!("{} is {} MB; Notey opens files up to {} MB.", path.display(), m.len() >> 20, self.size_limits().refuse >> 20),
                        )
                        .await;
                        return None;
                    }
                    SizeTier::VeryLarge => {
                        let a = ask(
                            &self.window,
                            "Open large file?",
                            &format!("{} is {} MB. Highlighting and match counting will be turned off.", path.display(), m.len() >> 20),
                            &[("cancel", "Cancel", Tone::Normal), ("open", "Open Anyway", Tone::Suggested)],
                            "open",
                            "cancel",
                        )
                        .await;
                        if a != "open" {
                            return None;
                        }
                    }
                    _ => {}
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                notify(&self.window, &format!("Can’t open {}", path.display()), &e.to_string()).await;
                return None;
            }
        }

        let loaded = if meta.is_ok() {
            let p = path.clone();
            match gio::spawn_blocking(move || file_io::read_file(&p, None)).await {
                Ok(Ok(l)) => Some(l),
                Ok(Err(e)) => {
                    notify(&self.window, &format!("Can’t open {}", path.display()), &e.to_string()).await;
                    return None;
                }
                Err(_) => return None,
            }
        } else {
            None
        };
        if let Some(l) = &loaded {
            if l.binary {
                let a = ask(
                    &self.window,
                    "This looks like a binary file",
                    "Opening it as text may show garbage. It will be opened read-only.",
                    &[("cancel", "Cancel", Tone::Normal), ("open", "Open Anyway", Tone::Destructive)],
                    "cancel",
                    "cancel",
                )
                .await;
                if a != "open" {
                    return None;
                }
            }
        }

        let reuse = self.current_doc().filter(|d| {
            d.path().is_none() && !d.is_dirty() && d.buffer.char_count() == 0 && !d.has_buffer_file.get()
        });
        let doc = Rc::new(Doc::new(&self.settings.borrow(), None));
        doc.set_path(Some(&path));
        match loaded {
            Some(l) => self.apply_loaded(&doc, l),
            None => {
                doc.detect_language();
                self.toast(&format!("New file: {}", path.display()));
            }
        }
        let loaded_tier = doc.state.borrow().tier;
        doc.state.borrow_mut().tier = tier.max_tier(loaded_tier);
        self.add_doc(&doc, true);
        if let Some(r) = reuse {
            r.force_close.set(true);
            if let Some(page) = self.page_of(&r) {
                self.tab_view.close_page(&page);
            }
        }
        self.after_open_notices(&doc);
        self.note_recent(&path);
        if let Some(l) = line {
            self.goto_line(&doc, l, None);
        }
        self.write_manifest_now();
        Some(doc)
    }

    fn after_open_notices(self: &Rc<Self>, doc: &Rc<Doc>) {
        let (guessed, errors, enc, tier) = {
            let st = doc.state.borrow();
            (st.guessed_encoding, st.had_errors, st.encoding, st.tier)
        };
        if guessed {
            self.show_info(
                doc,
                &format!("Opened as {} (guess). If the text looks wrong, reopen it with another encoding.", enc.label()),
                false,
                vec![("Reopen With Encoding…".into(), Box::new(|t, d| t.reopen_with_encoding_dialog(d)))],
            );
        }
        if errors {
            self.show_notice(doc, "Some bytes were not valid in this encoding and were replaced. Saving will write the replacement characters.");
        }
        if tier != SizeTier::Normal {
            self.show_notice(doc, "Large file: syntax highlighting and the minimap are off.");
        }
    }

    pub fn reopen_with_encoding_dialog(self: &Rc<Self>, doc: &Rc<Doc>) {
        self.choose_encoding(doc, true);
    }

    pub fn goto_line(&self, doc: &Rc<Doc>, line: u32, column: Option<u32>) {
        let last = (doc.buffer.line_count() - 1).max(0) as u32;
        let line0 = line.saturating_sub(1).min(last);
        let col0 = column.map_or(0, |c| c.saturating_sub(1));
        doc.set_cursor(crate::core::session::Cursor { line: line0, column: col0 });
        doc.center_on_cursor();
        doc.view.grab_focus();
    }

    fn note_recent(self: &Rc<Self>, path: &Path) {
        {
            let mut s = self.settings.borrow_mut();
            let mut r = crate::core::recent::RecentList { max: s.recent_size, items: std::mem::take(&mut s.recent_files) };
            r.push(&path.to_string_lossy());
            s.recent_files = r.items;
        }
        // Also tell the desktop (other apps' "Recent" lists). Skipped for throwaway profiles so
        // tests and trial runs do not write into the user's recently-used list.
        if self.persist_settings {
            let data = gtk::RecentData::new(None, None, "text/plain", "NoteyMcNoteface", "notey %u", &[], false);
            gtk::RecentManager::default().add_full(&gio::File::for_path(path).uri(), &data);
        }
        self.rebuild_recent_menu();
        self.save_settings();
    }

    pub fn rebuild_recent_menu(&self) {
        self.recent_menu.remove_all();
        let items = self.settings.borrow().recent_files.clone();
        let list = gio::Menu::new();
        for p in items {
            let name = Path::new(&p).file_name().map_or(p.clone(), |n| n.to_string_lossy().into_owned());
            let item = gio::MenuItem::new(Some(&name), None);
            item.set_action_and_target_value(Some("win.open-recent"), Some(&p.to_variant()));
            list.append_item(&item);
        }
        self.recent_menu.append_section(None, &list);
        let clear = gio::Menu::new();
        clear.append(Some("Clear Recent Files"), Some("win.clear-recent"));
        self.recent_menu.append_section(None, &clear);
    }

    // ----------------------------------------------------------------- reload

    pub fn spawn_reload(self: &Rc<Self>, doc: &Rc<Doc>, forced: Option<Encoding>) {
        let (this, doc) = (self.clone(), doc.clone());
        glib::spawn_future_local(async move { this.reload(&doc, forced).await });
    }

    /// Re-reads the file. `forced` is a user-chosen encoding ("Reopen with encoding").
    pub async fn reload(self: &Rc<Self>, doc: &Rc<Doc>, forced: Option<Encoding>) -> bool {
        let Some(path) = doc.path() else { return false };
        let line = doc.cursor();
        let top = doc.top_visible_line();
        let p = path.clone();
        match gio::spawn_blocking(move || file_io::read_file(&p, forced)).await {
            Ok(Ok(loaded)) => {
                self.apply_loaded(doc, loaded);
                doc.set_cursor(line);
                doc.scroll_to_line_top(top);
                doc.external_pending.set(false);
                while let Some(child) = doc.info.first_child() {
                    doc.info.remove(&child);
                }
                doc.info.set_visible(false);
                self.refresh_page(doc);
                self.snapshot(doc);
                self.write_manifest_now();
                self.update_status();
                true
            }
            Ok(Err(e)) => {
                notify(&self.window, &format!("Can’t read {}", path.display()), &e.to_string()).await;
                false
            }
            Err(_) => false,
        }
    }

    // ------------------------------------------------------------------ saving

    pub fn spawn_save(self: &Rc<Self>, doc: &Rc<Doc>, save_as: bool) {
        let (this, doc) = (self.clone(), doc.clone());
        glib::spawn_future_local(async move {
            this.save_doc(&doc, save_as).await;
        });
    }

    async fn pick_save_path(&self, doc: &Doc) -> Option<PathBuf> {
        let dialog = gtk::FileDialog::builder().title("Save As").modal(true).build();
        dialog.set_initial_name(Some(&doc.name()));
        if let Some(dir) = doc.path().and_then(|p| p.parent().map(Path::to_path_buf)) {
            dialog.set_initial_folder(Some(&gio::File::for_path(dir)));
        }
        let file = dialog.save_future(Some(&self.window)).await.ok()?;
        file.path()
    }

    /// Saves the document, asking for a path when it has none. Returns whether it was written.
    pub async fn save_doc(self: &Rc<Self>, doc: &Rc<Doc>, save_as: bool) -> bool {
        let read_only = doc.state.borrow().read_only;
        let path = match doc.path() {
            Some(p) if !save_as && !read_only => p,
            _ => {
                let Some(p) = self.pick_save_path(doc).await else { return false };
                let clash = self
                    .docs
                    .borrow()
                    .iter()
                    .any(|d| !Rc::ptr_eq(d, doc) && d.path().as_deref() == Some(p.as_path()));
                if clash {
                    notify(&self.window, "File is open in another tab", "Close that tab first, or choose another name.").await;
                    return false;
                }
                p
            }
        };

        let mut text = doc.text();
        if self.settings.borrow().ensure_final_newline && !text.is_empty() && !text.ends_with('\n') {
            doc.buffer.insert(&mut doc.buffer.end_iter(), "\n");
            text.push('\n');
        }
        let (enc, _bom, ending, mixed, had_errors) = {
            let st = doc.state.borrow();
            (st.encoding, st.bom, st.line_ending, st.mixed_endings, st.had_errors)
        };
        if mixed {
            let a = ask(
                &self.window,
                "Mixed line endings",
                &format!("This file mixes line ending styles. Saving will use {} everywhere.", ending.label()),
                &[("cancel", "Cancel", Tone::Normal), ("save", &format!("Save With {}", ending.label()), Tone::Suggested)],
                "save",
                "cancel",
            )
            .await;
            if a != "save" {
                return false;
            }
            doc.state.borrow_mut().mixed_endings = false;
        }
        if had_errors {
            let a = ask(
                &self.window,
                "Replacement characters will be saved",
                "Some bytes in this file were not valid and were shown as �. Saving writes those replacement characters over the originals.",
                &[("cancel", "Cancel", Tone::Normal), ("save", "Save Anyway", Tone::Destructive)],
                "cancel",
                "cancel",
            )
            .await;
            if a != "save" {
                return false;
            }
            doc.state.borrow_mut().had_errors = false;
        }
        let bad = encoding::check_convertible(&text, enc);
        if !bad.is_empty() {
            let sample = bad
                .iter()
                .take(10)
                .map(|u| format!("“{}” (line {}, column {})", u.ch, u.line, u.column))
                .collect::<Vec<_>>()
                .join("\n");
            let a = ask(
                &self.window,
                &format!("{} can’t represent {} character{}", enc.label(), bad.len(), if bad.len() == 1 { "" } else { "s" }),
                &format!("They would be saved as numeric references (&#1234;).\n\n{sample}"),
                &[
                    ("cancel", "Cancel", Tone::Normal),
                    ("save", "Save Anyway", Tone::Destructive),
                    ("utf8", "Save As UTF-8", Tone::Suggested),
                ],
                "utf8",
                "cancel",
            )
            .await;
            match a.as_str() {
                "save" => {}
                "utf8" => {
                    let mut st = doc.state.borrow_mut();
                    st.encoding = Encoding::Utf8;
                    st.bom = false;
                }
                _ => return false,
            }
        }
        let (enc, bom, ending) = {
            let st = doc.state.borrow();
            (st.encoding, st.bom, st.line_ending)
        };

        let (p, t) = (path.clone(), text);
        let result = gio::spawn_blocking(move || file_io::save_file(&p, &t, enc, bom, ending)).await;
        match result {
            Ok(Ok(disk)) => {
                let renamed = doc.path().as_deref() != Some(path.as_path());
                {
                    let mut st = doc.state.borrow_mut();
                    st.path = Some(path.clone());
                    st.disk = Some(disk);
                    st.read_only = false;
                }
                doc.view.set_editable(true);
                doc.buffer.set_modified(false);
                if renamed {
                    if doc.state.borrow().language_override.is_none() {
                        doc.detect_language();
                    }
                    self.watch(doc);
                    self.note_recent(&path);
                }
                self.refresh_page(doc);
                self.snapshot(doc);
                self.write_manifest_now();
                self.update_status();
                self.toast(&format!("Saved {}", doc.name()));
                true
            }
            Ok(Err(e)) => {
                notify(&self.window, &format!("Can’t save {}", path.display()), &e.to_string()).await;
                false
            }
            Err(_) => false,
        }
    }

    pub async fn save_all(self: &Rc<Self>) {
        for doc in self.docs_in_order() {
            if doc.is_dirty() && !self.save_doc(&doc, false).await {
                return;
            }
        }
    }

    // ----------------------------------------------------------------- watcher

    pub fn connect_doc_extras(self: &Rc<Self>, doc: &Rc<Doc>) {
        self.watch(doc);
    }

    pub fn watch(self: &Rc<Self>, doc: &Rc<Doc>) {
        if let Some(m) = doc.monitor.take() {
            m.cancel();
        }
        let Some(path) = doc.path() else { return };
        let Ok(monitor) = gio::File::for_path(&path).monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE) else {
            return;
        };
        let (weak, wd) = (Rc::downgrade(self), Rc::downgrade(doc));
        monitor.connect_changed(move |_, _, _, event| {
            use gio::FileMonitorEvent as E;
            if !matches!(event, E::ChangesDoneHint | E::Created | E::Deleted | E::Renamed | E::MovedIn | E::MovedOut) {
                return;
            }
            let (weak, wd) = (weak.clone(), wd.clone());
            glib::timeout_add_local_once(Duration::from_millis(250), move || {
                if let (Some(t), Some(d)) = (weak.upgrade(), wd.upgrade()) {
                    let t2 = t.clone();
                    glib::spawn_future_local(async move { t2.check_external(&d).await });
                }
            });
        });
        *doc.monitor.borrow_mut() = Some(monitor);
    }

    pub async fn check_external(self: &Rc<Self>, doc: &Rc<Doc>) {
        let Some(path) = doc.path() else { return };
        let p = path.clone();
        let Ok(now) = gio::spawn_blocking(move || file_io::disk_state_of(&p, None).ok()).await else { return };
        let known = doc.state.borrow().disk.clone();
        match reconcile(known.as_ref(), now.as_ref()) {
            Reconcile::InSync => {}
            Reconcile::Missing => {
                if known.is_some() {
                    self.show_external_change(doc, true);
                }
            }
            Reconcile::DiskChanged => {
                if known.is_none() && now.is_some() && !doc.is_dirty() {
                    // The file we were about to create now exists; just pick it up.
                    self.reload(doc, None).await;
                } else if doc.is_dirty() {
                    self.show_external_change(doc, false);
                } else if self.reload(doc, None).await {
                    self.toast(&format!("Reloaded {} (changed on disk)", doc.name()));
                }
            }
        }
    }

    // ------------------------------------------------------------- status bar

    pub fn update_status(&self) {
        let Some(doc) = self.current_doc() else { return };
        let it = doc.buffer.iter_at_offset(doc.buffer.cursor_position());
        let mut pos = format!("Ln {}, Col {}", it.line() + 1, it.line_offset() + 1);
        if let Some((a, b)) = doc.buffer.selection_bounds() {
            let chars = b.offset() - a.offset();
            let lines = b.line() - a.line() + 1;
            pos.push_str(&if lines > 1 { format!("  ({chars} chars, {lines} lines selected)") } else { format!("  ({chars} selected)") });
        }
        self.status.position.set_label(&pos);
        let st = doc.state.borrow();
        self.status.language.set_label(&doc.language_name());
        let enc = match (st.encoding, st.bom) {
            (Encoding::Utf8, true) => "UTF-8 BOM".to_string(),
            (e, _) => e.label(),
        };
        self.status.encoding.set_label(&enc);
        if st.guessed_encoding {
            self.status.encoding.add_css_class("guess");
        } else {
            self.status.encoding.remove_css_class("guess");
        }
        self.status.line_ending.set_label(&if st.mixed_endings { "Mixed".to_string() } else { st.line_ending.label().to_string() });
        let spaces = doc.view.is_insert_spaces_instead_of_tabs();
        self.status.indent.set_label(&format!("{}: {}", if spaces { "Spaces" } else { "Tabs" }, doc.view.indent_width()));
        let mut notes = Vec::new();
        if st.read_only {
            notes.push("Read-only");
        }
        if st.tier != SizeTier::Normal {
            notes.push("Large file");
        }
        self.status.note.set_label(&notes.join(" · "));
        drop(st);
        self.update_find_count();
    }

    // ------------------------------------------------------------------ actions

    fn add_action(self: &Rc<Self>, name: &str, f: impl Fn(&Rc<Notey>) + 'static) {
        let a = gio::SimpleAction::new(name, None);
        let weak = Rc::downgrade(self);
        a.connect_activate(move |_, _| {
            if let Some(t) = weak.upgrade() {
                f(&t);
            }
        });
        self.window.add_action(&a);
    }

    pub fn install_actions(self: &Rc<Self>) {
        self.add_action("new-tab", |t| {
            t.new_tab();
        });
        self.add_action("open", |t| {
            let t = t.clone();
            glib::spawn_future_local(async move {
                let dialog = gtk::FileDialog::builder().title("Open Files").modal(true).build();
                if let Some(dir) = t.current_doc().and_then(|d| d.path()).and_then(|p| p.parent().map(Path::to_path_buf)) {
                    dialog.set_initial_folder(Some(&gio::File::for_path(dir)));
                }
                if let Ok(model) = dialog.open_multiple_future(Some(&t.window)).await {
                    let paths: Vec<PathBuf> = (0..model.n_items())
                        .filter_map(|i| model.item(i).and_downcast::<gio::File>().and_then(|f| f.path()))
                        .collect();
                    for p in paths {
                        t.open_path(p, None).await;
                    }
                }
            });
        });
        let open_recent = gio::SimpleAction::new("open-recent", Some(glib::VariantTy::STRING));
        let weak = Rc::downgrade(self);
        open_recent.connect_activate(move |_, param| {
            if let (Some(t), Some(p)) = (weak.upgrade(), param.and_then(|p| p.get::<String>())) {
                if Path::new(&p).exists() {
                    t.open_in_background(vec![PathBuf::from(p)], None);
                } else {
                    let mut s = t.settings.borrow_mut();
                    s.recent_files.retain(|x| *x != p);
                    drop(s);
                    t.rebuild_recent_menu();
                    t.toast("That file no longer exists");
                }
            }
        });
        self.window.add_action(&open_recent);
        self.add_action("clear-recent", |t| {
            t.settings.borrow_mut().recent_files.clear();
            t.rebuild_recent_menu();
            t.save_settings();
        });
        self.add_action("save", |t| {
            if let Some(d) = t.current_doc() {
                t.spawn_save(&d, false);
            }
        });
        self.add_action("save-as", |t| {
            if let Some(d) = t.current_doc() {
                t.spawn_save(&d, true);
            }
        });
        self.add_action("save-all", |t| {
            let t = t.clone();
            glib::spawn_future_local(async move { t.save_all().await });
        });
        self.add_action("close-tab", |t| t.close_current());
        self.add_action("reopen-closed", |t| t.reopen_closed());
        self.add_action("reload", |t| {
            if let Some(d) = t.current_doc() {
                if d.path().is_none() {
                    return;
                }
                let (t, d) = (t.clone(), d);
                glib::spawn_future_local(async move {
                    if d.is_dirty() {
                        let a = ask(
                            &t.window,
                            "Reload from disk?",
                            "Your unsaved changes to this file will be lost.",
                            &[("cancel", "Cancel", Tone::Normal), ("reload", "Reload", Tone::Destructive)],
                            "cancel",
                            "cancel",
                        )
                        .await;
                        if a != "reload" {
                            return;
                        }
                    }
                    let forced = Some(d.state.borrow().encoding);
                    t.reload(&d, forced).await;
                });
            }
        });
        self.add_action("quit", |t| t.window.close());
        self.add_action("next-tab", |t| {
            t.tab_view.select_next_page();
        });
        self.add_action("prev-tab", |t| {
            t.tab_view.select_previous_page();
        });
        let goto = gio::SimpleAction::new("goto-tab", Some(glib::VariantTy::INT32));
        let weak = Rc::downgrade(self);
        goto.connect_activate(move |_, p| {
            if let (Some(t), Some(n)) = (weak.upgrade(), p.and_then(|p| p.get::<i32>())) {
                let n = if n == 9 { t.tab_view.n_pages() - 1 } else { n - 1 };
                if n >= 0 && n < t.tab_view.n_pages() {
                    t.tab_view.set_selected_page(&t.tab_view.nth_page(n));
                }
            }
        });
        self.window.add_action(&goto);
        self.install_editing_actions();
    }
}

trait MaxTier {
    fn max_tier(self, other: SizeTier) -> SizeTier;
}

impl MaxTier for SizeTier {
    fn max_tier(self, other: SizeTier) -> SizeTier {
        fn rank(t: SizeTier) -> u8 {
            match t {
                SizeTier::Normal => 0,
                SizeTier::Large => 1,
                SizeTier::VeryLarge => 2,
                SizeTier::Refuse => 3,
            }
        }
        if rank(self) >= rank(other) { self } else { other }
    }
}
