//! The application window: tabs, actions and the session.
use super::dialogs::{ask, Tone};
use super::doc::Doc;
use super::find_bar::FindBar;
use super::status_bar::StatusBar;
use crate::application::file_io;
use crate::application::session_store::{SessionStore, SessionWriter, Source};
use crate::core::encoding::Encoding;
use crate::core::session::{reconcile, Reconcile, Session, TabRecord, MAX_CLOSED_TABS};
use crate::settings::{Settings, ThemeChoice};
use adw::prelude::*;
use sourceview5::prelude::*;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

pub struct Options {
    pub session_dir: PathBuf,
    pub settings: Settings,
    /// Write settings back to disk (tests turn this off).
    pub persist_settings: bool,
}

impl Options {
    pub fn from_environment() -> Self {
        Self {
            session_dir: crate::application::session_store::default_dir(),
            settings: Settings::load(),
            persist_settings: true,
        }
    }
}

pub struct Notey {
    pub app: adw::Application,
    pub window: adw::ApplicationWindow,
    pub tab_view: adw::TabView,
    pub title: adw::WindowTitle,
    pub toasts: adw::ToastOverlay,
    pub status: StatusBar,
    pub find: FindBar,
    pub quickbar: gtk::ScrolledWindow,
    pub quick_key: RefCell<String>,
    pub prefs_page: RefCell<Option<(gtk::Widget, gtk::Stack)>>,
    pub docs: RefCell<Vec<Rc<Doc>>>,
    pub settings: RefCell<Settings>,
    pub writer: RefCell<Option<SessionWriter>>,
    pub reader: SessionStore,
    pub closed: RefCell<Vec<TabRecord>>,
    pub manifest_timer: RefCell<Option<glib::SourceId>>,
    pub quitting: Cell<bool>,
    pub quit_confirmed: Cell<bool>,
    pub shown_toasts: RefCell<Vec<adw::Toast>>,
    pub restoring: Cell<bool>,
    pub recent_menu: gio::Menu,
    pub editor_css: gtk::CssProvider,
    pub persist_settings: bool,
    pub context_page: RefCell<Option<adw::TabPage>>,
    pub weak_self: RefCell<std::rc::Weak<Notey>>,
}

pub type NoteyWindow = Rc<Notey>;

impl Notey {
    pub fn new(app: &adw::Application, opts: Options) -> NoteyWindow {
        if glib::application_name().is_none() {
            glib::set_application_name("NoteyMcNoteface");
        }
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("NoteyMcNoteface")
            .default_width(opts.settings.window_width)
            .default_height(opts.settings.window_height)
            .maximized(opts.settings.window_maximized)
            .build();
        let tab_view = adw::TabView::new();
        // Ctrl+Home/End (and with Shift) select the first/last tab by default, which would
        // steal document navigation from the editor.
        tab_view.remove_shortcuts(
            adw::TabViewShortcuts::CONTROL_HOME
                | adw::TabViewShortcuts::CONTROL_END
                | adw::TabViewShortcuts::CONTROL_SHIFT_HOME
                | adw::TabViewShortcuts::CONTROL_SHIFT_END,
        );
        let title = adw::WindowTitle::new("Notey", "");
        let toasts = adw::ToastOverlay::new();
        let editor_css = gtk::CssProvider::new();
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &editor_css,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
            );
        }

        let recent_menu = gio::Menu::new();
        let this = Rc::new(Notey {
            app: app.clone(),
            window,
            tab_view,
            title,
            toasts,
            status: StatusBar::new(),
            find: FindBar::new(),
            quickbar: super::menus::quick_actions(&opts.settings.quick_actions),
            prefs_page: RefCell::new(None),
            quick_key: RefCell::new(opts.settings.quick_actions.join(",")),
            docs: RefCell::new(Vec::new()),
            settings: RefCell::new(opts.settings.clone()),
            writer: RefCell::new(None),
            reader: SessionStore::new(opts.session_dir.clone()),
            closed: RefCell::new(Vec::new()),
            manifest_timer: RefCell::new(None),
            quitting: Cell::new(false),
            quit_confirmed: Cell::new(false),
            shown_toasts: RefCell::new(Vec::new()),
            restoring: Cell::new(true),
            recent_menu,
            editor_css,
            persist_settings: opts.persist_settings,
            context_page: RefCell::new(None),
            weak_self: RefCell::new(std::rc::Weak::new()),
        });
        *this.weak_self.borrow_mut() = Rc::downgrade(&this);

        this.build_ui();
        this.install_actions();
        this.install_find();
        this.install_status_menus();
        this.install_tab_menu();
        this.connect_signals();
        this.apply_css();
        this.rebuild_recent_menu();

        let mut store = SessionStore::new(opts.session_dir);
        let remember = this.settings.borrow().remember_tabs;
        let loaded = if remember { Some(store.load()) } else { None };
        if let Some(l) = &loaded {
            this.restore(l.session.clone(), &store);
            if let Source::Quarantined(dir) = &l.source {
                this.toast(&format!("Saved session was unreadable; kept a copy in {}", dir.display()));
            }
            if let Source::Newer(v) = &l.source {
                this.toast(&format!("Saved session is from a newer version ({v}); not modifying it"));
            }
        }
        if this.docs.borrow().is_empty() {
            this.new_tab();
        }
        store.remove_orphans(&this.build_session());
        *this.writer.borrow_mut() = Some(SessionWriter::start(store));
        this.restoring.set(false);
        this.write_manifest_now();
        this.start_periodic_flush();
        this
    }

    /// Dismisses every toast still showing (before the window is torn down).
    pub fn dismiss_toasts(&self) {
        for t in self.shown_toasts.borrow_mut().drain(..) {
            t.dismiss();
        }
    }

    pub fn toast(&self, text: &str) {
        let toast = adw::Toast::new(text);
        toast.set_use_markup(false);
        let mut shown = self.shown_toasts.borrow_mut();
        shown.retain(|t| t.title().is_some());
        shown.push(toast.clone());
        drop(shown);
        self.toasts.add_toast(toast);
    }

    // ------------------------------------------------------------------ layout

    fn build_ui(self: &Rc<Self>) {
        let header = adw::HeaderBar::new();
        header.set_title_widget(Some(&self.title));

        let new_btn = gtk::Button::from_icon_name("tab-new-symbolic");
        new_btn.set_tooltip_text(Some("New Tab (Ctrl+T)"));
        new_btn.set_action_name(Some("win.new-tab"));
        let open_btn = gtk::Button::from_icon_name("document-open-symbolic");
        open_btn.set_tooltip_text(Some("Open… (Ctrl+O)"));
        open_btn.set_action_name(Some("win.open"));
        header.pack_start(&new_btn);
        header.pack_start(&open_btn);

        let save_menu = gio::Menu::new();
        save_menu.append(Some("Save As…"), Some("win.save-as"));
        save_menu.append(Some("Save All"), Some("win.save-all"));
        let save_btn = adw::SplitButton::new();
        save_btn.set_icon_name("document-save-symbolic");
        save_btn.set_tooltip_text(Some("Save (Ctrl+S)"));
        save_btn.set_action_name(Some("win.save"));
        save_btn.set_menu_model(Some(&save_menu));
        header.pack_start(&save_btn);

        let menu = super::menus::main_menu_model(&self.recent_menu);
        let menu_btn = gtk::MenuButton::builder().icon_name("open-menu-symbolic").menu_model(&menu).primary(true).build();
        menu_btn.set_tooltip_text(Some("Main Menu"));
        header.pack_end(&menu_btn);

        let search_btn = gtk::ToggleButton::new();
        search_btn.set_icon_name("edit-find-symbolic");
        search_btn.set_tooltip_text(Some("Find and Replace (Ctrl+F)"));
        search_btn.set_action_name(Some("win.find-toggle"));
        header.pack_end(&search_btn);
        let prefs_btn = gtk::Button::from_icon_name("emblem-system-symbolic");
        prefs_btn.set_tooltip_text(Some("Preferences"));
        prefs_btn.set_action_name(Some("win.preferences"));
        header.pack_end(&prefs_btn);

        let tab_bar = adw::TabBar::new();
        tab_bar.set_view(Some(&self.tab_view));
        tab_bar.set_autohide(false);
        tab_bar.set_expand_tabs(false);

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.add_top_bar(&self.quickbar);
        toolbar.add_top_bar(&tab_bar);
        self.tab_view.set_vexpand(true);
        self.toasts.set_child(Some(&self.tab_view));
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&self.find.root);
        content.append(&self.toasts);
        self.toasts.set_vexpand(true);
        toolbar.set_content(Some(&content));
        toolbar.add_bottom_bar(&self.status.root);
        self.window.set_content(Some(&toolbar));
    }

    fn apply_css(&self) {
        let s = self.settings.borrow();
        let size = f64::from(s.font_size) * f64::from(s.zoom_percent) / 100.0;
        self.editor_css.load_from_string(&format!(
            ".notey-editor {{ font-family: \"{}\"; font-size: {size:.1}pt; }}",
            s.font.replace('"', "")
        ));
        let manager = adw::StyleManager::default();
        manager.set_color_scheme(match s.theme {
            ThemeChoice::System => adw::ColorScheme::Default,
            ThemeChoice::Light => adw::ColorScheme::ForceLight,
            ThemeChoice::Dark => adw::ColorScheme::ForceDark,
        });
        self.status.zoom.set_label(&format!("{}%", s.zoom_percent));
        drop(s);
        self.apply_chrome_visibility();
    }

    pub fn style_scheme(&self) -> Option<sourceview5::StyleScheme> {
        let manager = sourceview5::StyleSchemeManager::default();
        let dark = adw::StyleManager::default().is_dark();
        let chosen = self.settings.borrow().style_scheme.clone();
        chosen
            .and_then(|id| manager.scheme(&id))
            .or_else(|| manager.scheme(if dark { "Adwaita-dark" } else { "Adwaita" }))
            .or_else(|| manager.scheme(if dark { "oblivion" } else { "classic" }))
    }

    /// Re-applies settings to every open document and the chrome.
    pub fn apply_settings_everywhere(&self) {
        self.apply_css();
        let scheme = self.style_scheme();
        for d in self.docs.borrow().iter() {
            d.apply_settings(&self.settings.borrow());
            d.buffer.set_style_scheme(scheme.as_ref());
        }
        self.save_settings();
    }

    pub fn save_settings(&self) {
        if self.persist_settings {
            let _ = self.settings.borrow().save();
        }
    }

    // ----------------------------------------------------------------- signals

    fn connect_signals(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);

        self.tab_view.connect_selected_page_notify({
            let weak = weak.clone();
            move |_| {
                if let Some(this) = weak.upgrade() {
                    this.on_selected_changed();
                }
            }
        });
        self.tab_view.connect_close_page({
            let weak = weak.clone();
            move |_, page| {
                let Some(this) = weak.upgrade() else { return false };
                this.on_close_page(page)
            }
        });
        self.tab_view.connect_page_detached({
            let weak = weak.clone();
            move |_, page, _| {
                if let Some(this) = weak.upgrade() {
                    this.on_page_detached(page);
                }
            }
        });
        self.tab_view.connect_page_reordered({
            let weak = weak.clone();
            move |_, _, _| {
                if let Some(this) = weak.upgrade() {
                    this.write_manifest_now();
                }
            }
        });
        self.window.connect_close_request({
            let weak = weak.clone();
            move |w| {
                let Some(this) = weak.upgrade() else { return glib::Propagation::Proceed };
                if this.needs_quit_confirmation() {
                    this.confirm_quit();
                    return glib::Propagation::Stop;
                }
                this.remember_window_size(w);
                this.shutdown(false);
                glib::Propagation::Proceed
            }
        });
        let drop = gtk::DropTarget::new(gtk::gdk::FileList::static_type(), gtk::gdk::DragAction::COPY);
        drop.connect_drop({
            let weak = weak.clone();
            move |_, value, _, _| {
                let (Some(this), Ok(list)) = (weak.upgrade(), value.get::<gtk::gdk::FileList>()) else { return false };
                this.open_dropped(&list.files());
                true
            }
        });
        self.window.add_controller(drop);
        adw::StyleManager::default().connect_dark_notify({
            let weak = weak.clone();
            move |_| {
                if let Some(this) = weak.upgrade() {
                    let scheme = this.style_scheme();
                    for d in this.docs.borrow().iter() {
                        d.buffer.set_style_scheme(scheme.as_ref());
                    }
                }
            }
        });
        for sig in [libc::SIGTERM, libc::SIGHUP, libc::SIGINT] {
            let weak = weak.clone();
            glib::unix_signal_add_local(sig, move || {
                if let Some(this) = weak.upgrade() {
                    // A signal cannot ask; never throw unsaved text away on the way out.
                    this.shutdown(true);
                    this.app.quit();
                }
                glib::ControlFlow::Break
            });
        }
    }

    /// With "Remember tabs" off nothing keeps unsaved text, so quitting has to ask.
    pub fn needs_quit_confirmation(&self) -> bool {
        !self.settings.borrow().remember_tabs
            && !self.quit_confirmed.get()
            && self.docs.borrow().iter().any(|d| d.is_dirty())
    }

    fn confirm_quit(self: &Rc<Self>) {
        let this = self.clone();
        glib::spawn_future_local(async move {
            let dirty = this.docs.borrow().iter().filter(|d| d.is_dirty()).count();
            let answer = ask(
                &this.window,
                &format!("Quit with {dirty} unsaved tab{}?", if dirty == 1 { "" } else { "s" }),
                "Remember tabs is off, so unsaved text will be lost.",
                &[
                    ("cancel", "Cancel", Tone::Normal),
                    ("discard", "Discard and Quit", Tone::Destructive),
                    ("save", "Save All", Tone::Suggested),
                ],
                "save",
                "cancel",
            )
            .await;
            match answer.as_str() {
                "save" => {
                    this.save_all().await;
                    if this.docs.borrow().iter().any(|d| d.is_dirty()) {
                        return; // something was cancelled or failed; stay open
                    }
                }
                "discard" => {}
                _ => return,
            }
            this.quit_confirmed.set(true);
            this.window.close();
        });
    }

    fn remember_window_size(&self, w: &adw::ApplicationWindow) {
        let mut s = self.settings.borrow_mut();
        s.window_maximized = w.is_maximized();
        if !w.is_maximized() {
            s.window_width = w.width();
            s.window_height = w.height();
        }
    }

    pub fn find_doc(&self, child: &gtk::Widget) -> Option<Rc<Doc>> {
        self.docs.borrow().iter().find(|d| d.root.upcast_ref::<gtk::Widget>() == child).cloned()
    }

    pub fn current_doc(&self) -> Option<Rc<Doc>> {
        let page = self.tab_view.selected_page()?;
        self.find_doc(&page.child())
    }

    /// The tab page showing `child`, if it is (still) in the tab view. `TabView::page` itself
    /// returns a null object for a widget that is not a page, which the bindings turn into a panic.
    pub fn page_for_child(&self, child: &impl IsA<gtk::Widget>) -> Option<adw::TabPage> {
        let child = child.upcast_ref::<gtk::Widget>();
        (0..self.tab_view.n_pages()).map(|i| self.tab_view.nth_page(i)).find(|p| p.child() == *child)
    }

    pub fn page_of(&self, doc: &Doc) -> Option<adw::TabPage> {
        self.page_for_child(&doc.root)
    }

    pub fn doc_at(&self, pos: i32) -> Option<Rc<Doc>> {
        if pos < 0 || pos >= self.tab_view.n_pages() {
            return None;
        }
        self.find_doc(&self.tab_view.nth_page(pos).child())
    }

    pub fn docs_in_order(&self) -> Vec<Rc<Doc>> {
        (0..self.tab_view.n_pages()).filter_map(|i| self.doc_at(i)).collect()
    }

    // ------------------------------------------------------------------- tabs

    fn lowest_free_untitled(&self) -> u32 {
        let used: Vec<u32> = self
            .docs
            .borrow()
            .iter()
            .filter(|d| d.path().is_none())
            .map(|d| d.state.borrow().untitled_number)
            .collect();
        (1..).find(|n| !used.contains(n)).unwrap_or(1)
    }

    pub fn new_tab(self: &Rc<Self>) -> Rc<Doc> {
        let doc = Rc::new(Doc::new(&self.settings.borrow(), None));
        doc.state.borrow_mut().untitled_number = self.lowest_free_untitled();
        doc.detect_language();
        self.add_doc(&doc, true);
        doc
    }

    /// Puts the document in a tab and wires its signals.
    pub fn add_doc(self: &Rc<Self>, doc: &Rc<Doc>, select: bool) -> adw::TabPage {
        self.docs.borrow_mut().push(doc.clone());
        doc.buffer.set_style_scheme(self.style_scheme().as_ref());
        let page = self.tab_view.append(&doc.root);
        self.refresh_all_pages();
        let (wd, wt) = (Rc::downgrade(doc), Rc::downgrade(self));

        doc.buffer.connect_changed({
            let (wd, wt) = (wd.clone(), wt.clone());
            move |_| {
                if let (Some(d), Some(t)) = (wd.upgrade(), wt.upgrade()) {
                    d.generation.set(d.generation.get() + 1);
                    t.schedule_snapshot(&d);
                    t.update_status();
                }
            }
        });
        doc.buffer.connect_modified_changed({
            let (wd, wt) = (wd.clone(), wt.clone());
            move |_| {
                if let (Some(d), Some(t)) = (wd.upgrade(), wt.upgrade()) {
                    t.refresh_page(&d);
                    t.schedule_manifest();
                }
            }
        });
        doc.buffer.connect_cursor_position_notify({
            let (wd, wt) = (wd.clone(), wt.clone());
            move |_| {
                if let (Some(d), Some(t)) = (wd.upgrade(), wt.upgrade()) {
                    if t.current_doc().is_some_and(|c| Rc::ptr_eq(&c, &d)) {
                        t.update_status();
                    }
                    t.schedule_manifest();
                }
            }
        });
        doc.buffer.connect_mark_set({
            let (wd, wt) = (wd.clone(), wt.clone());
            move |_, _, mark| {
                if mark.name().as_deref() == Some("selection_bound") {
                    if let (Some(d), Some(t)) = (wd.upgrade(), wt.upgrade()) {
                        if t.current_doc().is_some_and(|c| Rc::ptr_eq(&c, &d)) {
                            t.update_status();
                        }
                    }
                }
            }
        });
        // Scroll position can only be applied once the view has a size.
        doc.view.connect_map({
            let wd = wd.clone();
            move |_| {
                if let Some(d) = wd.upgrade() {
                    if let Some(line) = d.pending_scroll.take() {
                        glib::idle_add_local_once(move || d.scroll_to_line_top(line));
                    }
                }
            }
        });
        self.connect_doc_extras(doc);
        self.on_doc_added_editing(doc);
        self.auto_close_hook(doc);
        self.on_doc_added_find(doc);
        if select {
            self.tab_view.set_selected_page(&page);
            doc.view.grab_focus();
        }
        self.schedule_manifest();
        page
    }

    /// Tab titles depend on their siblings (same names show their folder), so refresh them all.
    pub fn refresh_all_pages(&self) {
        for d in self.docs.borrow().iter() {
            self.refresh_page(d);
        }
    }

    pub fn refresh_page(&self, doc: &Doc) {
        let Some(page) = self.page_of(doc) else { return };
        let name = doc.name();
        let dirty = doc.is_dirty();
        let siblings = self.docs.borrow().iter().filter(|d| d.name() == name).count();
        let label = match (siblings > 1, doc.path()) {
            (true, Some(p)) => {
                let parent = p.parent().and_then(|d| d.file_name()).map(|n| n.to_string_lossy().into_owned());
                match parent {
                    Some(par) => format!("{name} — {par}"),
                    None => name.clone(),
                }
            }
            _ => name.clone(),
        };
        page.set_title(&if dirty { format!("• {label}") } else { label });
        // The tooltip is Pango markup: a path like `R&D.txt` must be escaped.
        page.set_tooltip(&glib::markup_escape_text(&doc.path().map_or_else(|| name.clone(), |p| p.display().to_string())));
        page.set_needs_attention(false);
        if self.tab_view.selected_page().as_ref() == Some(&page) {
            self.update_window_title(doc);
        }
    }

    fn update_window_title(&self, doc: &Doc) {
        self.title.set_title(&format!("{}{}", if doc.is_dirty() { "• " } else { "" }, doc.name()));
        let sub = match doc.path() {
            Some(p) => p.parent().map(|d| d.display().to_string()).unwrap_or_default(),
            None => "Unsaved".into(),
        };
        self.title.set_subtitle(&sub);
        self.window.set_title(Some(&format!("{} — NoteyMcNoteface", doc.name())));
    }

    fn on_selected_changed(self: &Rc<Self>) {
        if let Some(doc) = self.current_doc() {
            self.update_window_title(&doc);
            self.update_status();
            self.on_doc_selected(&doc);
            self.schedule_manifest();
        } else if self.tab_view.selected_page().is_some() {
            // A non-document page such as Preferences.
            self.title.set_title("Preferences");
            self.title.set_subtitle("");
            self.window.set_title(Some("Preferences — NoteyMcNoteface"));
            self.status.position.set_label("");
        }
    }

    // ----------------------------------------------------------- closing tabs

    fn on_close_page(self: &Rc<Self>, page: &adw::TabPage) -> bool {
        let Some(doc) = self.find_doc(&page.child()) else { return false };
        if !doc.is_dirty() || doc.force_close.get() || self.quitting.get() {
            self.remember_closed(&doc);
            self.tab_view.close_page_finish(page, true);
            return true;
        }
        let (this, page, doc) = (self.clone(), page.clone(), doc);
        glib::spawn_future_local(async move {
            // The page is already selected by the user's click; make sure it is visible for context.
            this.tab_view.set_selected_page(&page);
            let answer = ask(
                &this.window,
                &format!("Save changes to “{}”?", doc.name()),
                "Your changes will be lost if you discard them.",
                &[
                    ("cancel", "Cancel", Tone::Normal),
                    ("discard", "Discard", Tone::Destructive),
                    ("save", "Save", Tone::Suggested),
                ],
                "save",
                "cancel",
            )
            .await;
            let confirm = match answer.as_str() {
                "save" => this.save_doc(&doc, false).await,
                "discard" => true,
                _ => false,
            };
            if confirm {
                this.remember_closed(&doc);
            }
            // Every path out of the prompt answers the tab view, or the tab would hang.
            this.tab_view.close_page_finish(&page, confirm);
        });
        true
    }

    /// Keeps the closed tab (and its unsaved text) for "Reopen Closed Tab".
    fn remember_closed(&self, doc: &Rc<Doc>) {
        if self.quitting.get() || self.restoring.get() {
            return;
        }
        self.snapshot(doc);
        let title = doc.name();
        let rec = doc.record(&title, false);
        let worth_keeping = rec.path.is_some() || doc.has_buffer_file.get();
        if !worth_keeping {
            return;
        }
        let mut closed = self.closed.borrow_mut();
        closed.retain(|r| r.id != rec.id);
        closed.insert(0, rec);
        while closed.len() > MAX_CLOSED_TABS {
            if let Some(old) = closed.pop() {
                if let (Some(file), Some(w)) = (old.buffer_file, self.writer.borrow().as_ref()) {
                    w.remove_buffer(file);
                }
            }
        }
    }

    fn on_page_detached(self: &Rc<Self>, page: &adw::TabPage) {
        let child = page.child();
        let removed = {
            let mut docs = self.docs.borrow_mut();
            docs.iter().position(|d| d.root.upcast_ref::<gtk::Widget>() == &child).map(|i| docs.remove(i))
        };
        if let Some(doc) = removed {
            if let Some(id) = doc.snapshot_timer.take() {
                id.remove();
            }
            if let Some(m) = doc.monitor.take() {
                m.cancel();
            }
            // A closed tab's buffer file is now referenced only by `closed`; a plain
            // clean tab has none. Either way nothing is deleted here.
        }
        if self.quitting.get() {
            return;
        }
        self.refresh_all_pages();
        if self.docs.borrow().is_empty() {
            self.new_tab();
        }
        self.write_manifest_now();
    }

    pub fn reopen_closed(self: &Rc<Self>) {
        let rec = {
            let mut closed = self.closed.borrow_mut();
            if closed.is_empty() {
                return;
            }
            closed.remove(0)
        };
        // The tab's text was handed to the writer thread when it closed; make sure it is on disk
        // before reading it back, or a quick reopen would find nothing there.
        if let Some(w) = self.writer.borrow().as_ref() {
            w.flush();
        }
        let buffer = rec.buffer_file.as_ref().map(|f| self.reader.read_buffer(f).ok());
        let doc = self.restore_tab(&rec, buffer);
        let page = self.add_doc(&doc, true);
        self.tab_view.set_page_pinned(&page, rec.pinned);
        self.write_manifest_now();
    }

    pub fn close_current(&self) {
        if let Some(page) = self.tab_view.selected_page() {
            self.tab_view.close_page(&page);
        }
    }

    // ---------------------------------------------------------------- session

    pub fn build_session(&self) -> Session {
        let saved_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let mut s = Session { saved_at, ..Session::default() };
        for i in 0..self.tab_view.n_pages() {
            let page = self.tab_view.nth_page(i);
            if let Some(doc) = self.find_doc(&page.child()) {
                s.tabs.push(doc.record(&doc.name(), page.is_pinned()));
            }
        }
        s.active_tab = self.current_doc().map(|d| d.id.clone());
        s.closed_tabs = self.closed.borrow().clone();
        s
    }

    /// "Clear saved session": forget what is stored, but keep every open tab's unsaved text safe by
    /// writing it again straight away. Going through the writer thread keeps the order right.
    pub fn clear_saved_session(&self) {
        if let Some(w) = self.writer.borrow().as_ref() {
            w.clear();
        }
        for d in self.docs.borrow().iter() {
            d.has_buffer_file.set(false); // so the next snapshot writes the text again
        }
        self.snapshot_all();
        self.write_manifest_now();
    }

    /// True when the manifest would include a tab that is not a document (it never should).
    pub fn session_has_non_documents(&self) -> bool {
        self.build_session().tabs.len() != self.docs.borrow().len()
    }

    pub fn write_manifest_now(&self) {
        if self.restoring.get() {
            return;
        }
        if let Some(id) = self.manifest_timer.take() {
            id.remove();
        }
        let session = self.build_session();
        if let Some(w) = self.writer.borrow().as_ref() {
            w.write_manifest(session);
        }
    }

    pub fn schedule_manifest(self: &Rc<Self>) {
        if self.restoring.get() || self.manifest_timer.borrow().is_some() {
            return;
        }
        let weak = Rc::downgrade(self);
        let id = glib::timeout_add_local_once(Duration::from_secs(2), move || {
            if let Some(this) = weak.upgrade() {
                *this.manifest_timer.borrow_mut() = None;
                this.write_manifest_now();
            }
        });
        *self.manifest_timer.borrow_mut() = Some(id);
    }

    /// Debounced: one second after the last edit.
    pub fn schedule_snapshot(self: &Rc<Self>, doc: &Rc<Doc>) {
        if self.restoring.get() {
            return;
        }
        if let Some(id) = doc.snapshot_timer.take() {
            id.remove();
        }
        let (weak, wd) = (Rc::downgrade(self), Rc::downgrade(doc));
        let id = glib::timeout_add_local_once(Duration::from_secs(1), move || {
            if let (Some(this), Some(doc)) = (weak.upgrade(), wd.upgrade()) {
                *doc.snapshot_timer.borrow_mut() = None;
                this.snapshot(&doc);
            }
        });
        *doc.snapshot_timer.borrow_mut() = Some(id);
    }

    /// Writes the tab's unsaved text if it changed. The buffer file is always written
    /// before a manifest that mentions it, and a manifest that drops it is written
    /// before the file is removed, so a crash at any point leaves recoverable text.
    pub fn snapshot(&self, doc: &Rc<Doc>) {
        let writer = self.writer.borrow();
        let Some(writer) = writer.as_ref() else { return };
        let file = TabRecord::buffer_file_name(&doc.id);
        if doc.needs_buffer_file() {
            let gen = doc.generation.get();
            let first = !doc.has_buffer_file.get();
            if first || gen != doc.snapshot_generation.get() {
                writer.write_buffer(file, doc.text());
                doc.snapshot_generation.set(gen);
            }
            if first {
                doc.has_buffer_file.set(true);
                writer.write_manifest(self.build_session());
            }
        } else if doc.has_buffer_file.get() {
            doc.has_buffer_file.set(false);
            writer.write_manifest(self.build_session());
            writer.remove_buffer(file);
        }
    }

    pub fn snapshot_all(&self) {
        for d in self.docs.borrow().clone() {
            self.snapshot(&d);
        }
    }

    fn start_periodic_flush(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        glib::timeout_add_seconds_local(30, move || {
            let Some(this) = weak.upgrade() else { return glib::ControlFlow::Break };
            if this.quitting.get() {
                return glib::ControlFlow::Break;
            }
            this.snapshot_all();
            this.write_manifest_now();
            glib::ControlFlow::Continue
        });
    }

    /// Final synchronous flush; idempotent. After this nothing else is written, so
    /// teardown handlers cannot overwrite the session with an empty one.
    pub fn shutdown(&self, keep_unsaved: bool) {
        if self.quitting.replace(true) {
            return;
        }
        let any_dirty = self.docs.borrow().iter().any(|d| d.is_dirty());
        let remember = self.settings.borrow().remember_tabs || (keep_unsaved && any_dirty);
        let writer = self.writer.borrow_mut().take();
        if let Some(mut w) = writer {
            if remember {
                for d in self.docs.borrow().clone() {
                    let file = TabRecord::buffer_file_name(&d.id);
                    if d.needs_buffer_file() {
                        let gen = d.generation.get();
                        if !d.has_buffer_file.get() || gen != d.snapshot_generation.get() {
                            w.write_buffer(file, d.text());
                            d.has_buffer_file.set(true);
                        }
                    } else if d.has_buffer_file.get() {
                        d.has_buffer_file.set(false);
                    }
                }
                w.write_manifest(self.build_session());
            }
            let errors = w.shutdown();
            if !errors.is_empty() {
                eprintln!("notey: could not write session: {}", errors.join("; "));
            }
            if !remember {
                SessionStore::new(self.reader.dir().to_path_buf()).clear();
            }
        }
        self.save_settings();
    }

    fn restore(self: &Rc<Self>, session: Session, store: &SessionStore) {
        let buffers = store.read_all_buffers(&session);
        *self.closed.borrow_mut() = session.closed_tabs.clone();
        let mut active = None;
        for rec in &session.tabs {
            let buffer = rec.buffer_file.as_ref().map(|_| buffers.get(&rec.id).cloned().flatten());
            let doc = self.restore_tab(rec, buffer);
            let page = self.add_doc(&doc, false);
            self.tab_view.set_page_pinned(&page, rec.pinned);
            if session.active_tab.as_deref() == Some(rec.id.as_str()) {
                active = Some(page);
            }
        }
        if let Some(p) = active.or_else(|| (self.tab_view.n_pages() > 0).then(|| self.tab_view.nth_page(0))) {
            self.tab_view.set_selected_page(&p);
        }
    }

    /// Rebuilds a document from its record. `buffer`: `None` = the record has no buffer file,
    /// `Some(None)` = it has one that could not be read, `Some(Some(text))` = recovered text.
    pub fn restore_tab(self: &Rc<Self>, rec: &TabRecord, buffer: Option<Option<String>>) -> Rc<Doc> {
        let doc = Rc::new(Doc::new(&self.settings.borrow(), Some(rec.id.clone())));
        {
            let mut st = doc.state.borrow_mut();
            st.encoding = Encoding::from_name(&rec.encoding).unwrap_or(Encoding::Utf8);
            st.bom = rec.bom;
            st.line_ending = rec.line_ending;
            st.language_override = rec.language.clone();
            st.path = rec.path.as_ref().map(PathBuf::from);
            st.disk = rec.base.clone();
            if st.path.is_none() {
                drop(st);
                doc.state.borrow_mut().untitled_number = self.lowest_free_untitled();
            }
        }
        let path = doc.path();
        match (&buffer, &path) {
            (Some(Some(text)), _) => {
                doc.set_text(text, rec.dirty || path.is_none() && !text.is_empty());
                doc.has_buffer_file.set(true);
                if let Some(p) = &path {
                    let now = file_io::disk_state_of(p, None).ok();
                    match reconcile(rec.base.as_ref(), now.as_ref()) {
                        Reconcile::InSync => {}
                        Reconcile::DiskChanged => self.show_external_change(&doc, false),
                        Reconcile::Missing => self.show_external_change(&doc, true),
                    }
                }
            }
            (_, Some(p)) => {
                // Clean file tab, or a dirty one whose buffer file was lost.
                let forced = Encoding::from_name(&rec.encoding);
                match file_io::read_file(p, forced) {
                    Ok(loaded) => {
                        self.apply_loaded(&doc, loaded);
                        if buffer.is_some() {
                            self.show_notice(&doc, "Unsaved changes to this file could not be recovered; showing the file on disk.");
                        }
                    }
                    Err(e) => {
                        doc.set_text("", false);
                        self.show_notice(&doc, &format!("Could not read {}: {e}", p.display()));
                    }
                }
            }
            (Some(None), None) => {
                self.show_notice(&doc, "The text of this tab could not be recovered.");
            }
            (None, None) => {}
        }
        doc.detect_language();
        doc.set_cursor(rec.cursor);
        if let Some(sel) = rec.selection {
            doc.set_selection(sel);
        }
        doc.pending_scroll.set(Some(rec.scroll_line));
        doc.generation.set(0);
        doc.snapshot_generation.set(0);
        doc
    }

    fn on_doc_selected(self: &Rc<Self>, doc: &Rc<Doc>) {
        let _ = doc;
        self.run_search();
    }
}
