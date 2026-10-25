//! Preferences, laid out like Zorin Explorer's: one in-window "Preferences" tab with a
//! category switcher, each category a plain page of checks, radios, spin buttons and buttons.
use super::menus::{move_entry, nth_known, quick_action, QUICK_ACTIONS, SEPARATOR};
use super::window::Notey;
use crate::core::encoding::Encoding;
use crate::core::line_ending::LineEnding;
use crate::settings::{Settings, ThemeChoice, MAX_INDENT_WIDTH, MIN_INDENT_WIDTH};
use adw::prelude::*;
use gtk::{Box as GtkBox, Button, CheckButton, DropDown, Entry, Label, Orientation, SpinButton};
use std::rc::Rc;

pub const CATEGORIES: [&str; 8] = [
    "General",
    "Editor",
    "Indentation",
    "Appearance",
    "Files",
    "Quick actions",
    "Search & privacy",
    "About",
];

const APP_DESKTOP_ID: &str = "com.zorin.Notey.desktop";

/// File types Notey can be the default app for, grouped so one switch covers a family of
/// MIME types. Every entry must also be in the MimeType= line of the desktop file.
const FILE_TYPE_GROUPS: [(&str, &[&str]); 9] = [
    ("Plain text", &["text/plain", "text/x-log", "text/x-readme", "text/x-changelog", "text/x-copying", "text/x-authors", "text/x-install"]),
    ("Markdown", &["text/markdown"]),
    ("Source code", &["text/x-csrc", "text/x-chdr", "text/x-c++src", "text/x-python", "text/x-script.python", "text/x-rust", "text/x-java", "text/x-go", "text/x-lua", "text/x-perl", "text/x-ruby", "text/x-php", "text/x-sql", "application/sql", "text/x-tex", "text/x-cmake", "text/x-makefile"]),
    ("Shell scripts", &["text/x-shellscript", "application/x-shellscript"]),
    ("JavaScript and TypeScript", &["text/javascript", "text/x-javascript", "application/javascript", "application/x-javascript", "text/jsx", "text/x-typescript"]),
    ("JSON, YAML and TOML", &["application/json", "application/ld+json", "application/x-yaml", "text/x-toml", "application/toml"]),
    ("XML and HTML", &["text/xml", "application/xml", "text/html", "application/xhtml+xml"]),
    ("Config and stylesheets", &["text/x-ini", "application/x-ini", "application/x-desktop", "text/css", "text/x-scss", "text/x-sass", "text/x-less"]),
    ("Diffs and tables", &["text/x-diff", "text/x-patch", "text/csv", "text/x-csv", "text/tab-separated-values"]),
];

const MARGINS: [u32; 5] = [0, 72, 80, 100, 120];

fn slug(category: &str) -> String {
    category.to_lowercase().replace(' ', "-").replace('&', "and")
}

fn page(title: &str) -> GtkBox {
    let page = GtkBox::new(Orientation::Vertical, 10);
    page.set_margin_top(18);
    let heading = Label::new(Some(title));
    heading.add_css_class("title-3");
    heading.set_xalign(0.0);
    page.append(&heading);
    page
}

fn heading(text: &str) -> Label {
    let l = Label::new(Some(text));
    l.add_css_class("heading");
    l.set_xalign(0.0);
    l.set_margin_top(12);
    l
}

fn note(text: &str) -> Label {
    let l = Label::new(Some(text));
    l.add_css_class("dim-label");
    l.set_wrap(true);
    l.set_xalign(0.0);
    l
}

fn row_label(text: &str) -> Label {
    let l = Label::new(Some(text));
    l.set_hexpand(true);
    l.set_xalign(0.0);
    l
}

fn accel_label(accel: &str) -> String {
    match gtk::accelerator_parse(accel) {
        Some((key, mods)) => gtk::accelerator_get_label(key, mods).to_string(),
        None => accel.to_string(),
    }
}

type QuickEdit = Box<dyn Fn(&mut Vec<String>)>;
type BoolGet = fn(&Settings) -> bool;
type BoolSet = fn(&mut Settings, bool);

impl Notey {
    fn pref_check(self: &Rc<Self>, page: &GtkBox, label: &str, get: BoolGet, set: BoolSet) -> CheckButton {
        let check = CheckButton::with_label(label);
        check.set_active(get(&self.settings.borrow()));
        let weak = Rc::downgrade(self);
        check.connect_toggled(move |c| {
            if let Some(t) = weak.upgrade() {
                set(&mut t.settings.borrow_mut(), c.is_active());
                t.apply_settings_everywhere();
            }
        });
        page.append(&check);
        check
    }

    fn pref_spin(self: &Rc<Self>, page: &GtkBox, label: &str, (min, max): (u32, u32), get: fn(&Settings) -> u32, set: fn(&mut Settings, u32)) -> SpinButton {
        let row = GtkBox::new(Orientation::Horizontal, 8);
        row.append(&row_label(label));
        let adj = gtk::Adjustment::new(f64::from(get(&self.settings.borrow())), f64::from(min), f64::from(max), 1.0, 4.0, 0.0);
        let spin = SpinButton::new(Some(&adj), 1.0, 0);
        let weak = Rc::downgrade(self);
        spin.connect_value_changed(move |s| {
            if let Some(t) = weak.upgrade() {
                set(&mut t.settings.borrow_mut(), s.value() as u32);
                t.apply_settings_everywhere();
            }
        });
        row.append(&spin);
        page.append(&row);
        spin
    }

    /// One radio button per item; `current` picks the active one.
    fn pref_radio<T: Copy + PartialEq + 'static>(self: &Rc<Self>, page: &GtkBox, items: &[(&str, T)], current: T, set: impl Fn(&mut Settings, T) + 'static) {
        let set = Rc::new(set);
        let mut first: Option<CheckButton> = None;
        for (label, value) in items {
            let control = CheckButton::with_label(label);
            control.set_active(*value == current);
            match &first {
                Some(f) => control.set_group(Some(f)),
                None => first = Some(control.clone()),
            }
            let (weak, set, value) = (Rc::downgrade(self), set.clone(), *value);
            control.connect_toggled(move |c| {
                if let (true, Some(t)) = (c.is_active(), weak.upgrade()) {
                    set(&mut t.settings.borrow_mut(), value);
                    t.apply_settings_everywhere();
                }
            });
            page.append(&control);
        }
    }

    fn pref_dropdown(self: &Rc<Self>, page: &GtkBox, label: &str, items: &[String], selected: u32, set: impl Fn(&mut Settings, u32) + 'static) {
        let row = GtkBox::new(Orientation::Horizontal, 8);
        row.append(&row_label(label));
        let refs: Vec<&str> = items.iter().map(String::as_str).collect();
        let dd = DropDown::from_strings(&refs);
        dd.set_selected(selected);
        let weak = Rc::downgrade(self);
        dd.connect_selected_notify(move |d| {
            if let Some(t) = weak.upgrade() {
                set(&mut t.settings.borrow_mut(), d.selected());
                t.apply_settings_everywhere();
            }
        });
        row.append(&dd);
        page.append(&row);
    }

    fn button(label: &str, on_click: impl Fn() + 'static) -> Button {
        let b = Button::with_label(label);
        b.set_halign(gtk::Align::Start);
        b.connect_clicked(move |_| on_click());
        b
    }

    pub fn show_preferences(self: &Rc<Self>) {
        self.show_preferences_page(None);
    }

    /// Opens (or selects) the single Preferences tab, optionally on a category.
    pub fn show_preferences_page(self: &Rc<Self>, category: Option<&str>) {
        let existing = self.prefs_page.borrow().clone();
        let (page, stack) = match existing.and_then(|(child, stack)| self.page_for_child(&child).map(|p| (p, stack))) {
            Some(found) => found,
            None => {
                let (scroll, stack) = self.build_preferences();
                let page = self.tab_view.append(&scroll);
                page.set_title("Preferences");
                page.set_icon(Some(&gio::ThemedIcon::new("emblem-system-symbolic")));
                *self.prefs_page.borrow_mut() = Some((scroll.upcast(), stack.clone()));
                (page, stack)
            }
        };
        if let Some(c) = category {
            stack.set_visible_child_name(c);
        }
        self.tab_view.set_selected_page(&page);
    }

    fn build_preferences(self: &Rc<Self>) -> (gtk::ScrolledWindow, gtk::Stack) {
        let content = GtkBox::new(Orientation::Vertical, 14);
        content.set_margin_top(24);
        content.set_margin_bottom(24);
        content.set_margin_start(32);
        content.set_margin_end(32);
        let title = Label::new(Some("Preferences"));
        title.add_css_class("title-1");
        title.set_xalign(0.0);
        content.append(&title);
        content.append(&note("Changes apply to every open tab straight away and are remembered."));
        let stack = gtk::Stack::new();
        stack.set_vexpand(true);
        stack.set_hexpand(true);
        let switcher = gtk::StackSwitcher::new();
        switcher.set_stack(Some(&stack));
        switcher.set_halign(gtk::Align::Start);
        // The eight categories are wider than a narrow window; scroll them rather than the page.
        let switcher_scroll = gtk::ScrolledWindow::builder().child(&switcher).hscrollbar_policy(gtk::PolicyType::Automatic).vscrollbar_policy(gtk::PolicyType::Never).build();
        content.append(&switcher_scroll);
        for category in CATEGORIES {
            let p = match category {
                "General" => self.general_page(),
                "Editor" => self.editor_page(),
                "Indentation" => self.indentation_page(),
                "Appearance" => self.appearance_page(),
                "Files" => self.files_page(),
                "Quick actions" => self.quick_actions_page(),
                "Search & privacy" => self.privacy_page(),
                _ => self.about_page(),
            };
            stack.add_titled(&p, Some(&slug(category)), category);
        }
        content.append(&stack);
        let scroll = gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).build();
        scroll.set_child(Some(&content));
        (scroll, stack)
    }

    fn general_page(self: &Rc<Self>) -> GtkBox {
        let p = page("General");
        self.pref_check(&p, "Restore previous tabs on launch", |s| s.remember_tabs, |s, v| s.remember_tabs = v);
        p.append(&note("Unsaved text is kept in a private session copy and restored, but never written to your files. With this off, quitting asks before discarding unsaved tabs."));
        self.pref_check(&p, "Cut or copy the whole line when nothing is selected", |s| s.cut_line_when_empty, |s, v| s.cut_line_when_empty = v);
        self.pref_check(&p, "Ensure a newline at the end of the file when saving", |s| s.ensure_final_newline, |s, v| s.ensure_final_newline = v);
        p.append(&heading("Recent files"));
        self.pref_spin(&p, "Number of recent files to remember", (1, 50), |s| s.recent_size as u32, |s, v| s.recent_size = v as usize);
        let weak = Rc::downgrade(self);
        p.append(&Self::button("Clear recent files", move || {
            if let Some(t) = weak.upgrade() {
                t.settings.borrow_mut().recent_files.clear();
                t.rebuild_recent_menu();
                t.save_settings();
            }
        }));
        p
    }

    fn editor_page(self: &Rc<Self>) -> GtkBox {
        let p = page("Editor");
        p.append(&heading("Text"));
        let row = GtkBox::new(Orientation::Horizontal, 8);
        row.append(&row_label("Font family"));
        let font = Entry::new();
        font.set_width_chars(20);
        font.set_text(&self.settings.borrow().font);
        let weak = Rc::downgrade(self);
        font.connect_changed(move |e| {
            if let (false, Some(t)) = (e.text().is_empty(), weak.upgrade()) {
                t.settings.borrow_mut().font = e.text().to_string();
                t.apply_settings_everywhere();
            }
        });
        row.append(&font);
        p.append(&row);
        self.pref_spin(&p, "Font size (points, before zoom)", (6, 72), |s| s.font_size, |s, v| s.font_size = v);
        p.append(&heading("Display"));
        self.pref_check(&p, "Word wrap", |s| s.word_wrap, |s, v| s.word_wrap = v);
        self.pref_check(&p, "Line numbers", |s| s.line_numbers, |s, v| s.line_numbers = v);
        self.pref_check(&p, "Highlight the current line", |s| s.highlight_current_line, |s, v| s.highlight_current_line = v);
        self.pref_check(&p, "Show spaces and tabs", |s| s.show_whitespace, |s, v| s.show_whitespace = v);
        self.pref_check(&p, "Minimap", |s| s.show_minimap, |s, v| s.show_minimap = v);
        p.append(&heading("Right margin guide"));
        let labels: Vec<String> = MARGINS.iter().map(|m| if *m == 0 { "Off".into() } else { format!("{m} columns") }).collect();
        let current = self.settings.borrow().right_margin;
        let mut first: Option<CheckButton> = None;
        for (label, value) in labels.iter().zip(MARGINS) {
            let c = CheckButton::with_label(label);
            c.set_active(value == current);
            match &first {
                Some(f) => c.set_group(Some(f)),
                None => first = Some(c.clone()),
            }
            let weak = Rc::downgrade(self);
            c.connect_toggled(move |c| {
                if let (true, Some(t)) = (c.is_active(), weak.upgrade()) {
                    t.settings.borrow_mut().right_margin = value;
                    t.apply_settings_everywhere();
                }
            });
            p.append(&c);
        }
        p
    }

    fn indentation_page(self: &Rc<Self>) -> GtkBox {
        let p = page("Indentation");
        p.append(&note("Defaults for new tabs. A file whose existing indentation is recognised keeps its own style; change it for one tab from the status bar."));
        self.pref_spin(&p, "Indent width (columns per level and per tab stop)", (MIN_INDENT_WIDTH, MAX_INDENT_WIDTH), |s| s.indent_width, |s, v| s.indent_width = v);
        self.pref_check(&p, "Insert spaces instead of tabs", |s| s.insert_spaces, |s, v| s.insert_spaces = v);
        self.pref_check(&p, "Keep the previous line’s indentation on Enter", |s| s.auto_indent, |s, v| s.auto_indent = v);
        self.pref_check(&p, "Close brackets and quotes automatically", |s| s.auto_close_brackets, |s, v| s.auto_close_brackets = v);
        p
    }

    fn appearance_page(self: &Rc<Self>) -> GtkBox {
        let p = page("Appearance");
        p.append(&heading("Color scheme"));
        let current = self.settings.borrow().theme;
        self.pref_radio(&p, &[("Follow the system", ThemeChoice::System), ("Light", ThemeChoice::Light), ("Dark", ThemeChoice::Dark)], current, |s, v| s.theme = v);
        p.append(&heading("Syntax colors"));
        p.append(&note("Automatic picks a light or dark style to match the color scheme."));
        let manager = sourceview5::StyleSchemeManager::default();
        let mut ids: Vec<String> = manager.scheme_ids().iter().map(|g| g.to_string()).collect();
        ids.sort();
        let mut names = vec!["Automatic".to_string()];
        names.extend(ids.iter().map(|id| manager.scheme(id).map_or(id.clone(), |sc| sc.name().to_string())));
        let chosen = self.settings.borrow().style_scheme.as_ref().and_then(|id| ids.iter().position(|x| x == id)).map_or(0, |i| i as u32 + 1);
        self.pref_dropdown(&p, "Style", &names, chosen, move |s, i| s.style_scheme = (i > 0).then(|| ids[i as usize - 1].clone()));
        p
    }

    fn files_page(self: &Rc<Self>) -> GtkBox {
        let p = page("Files");
        p.append(&note("What new files are saved as. Existing files keep the encoding and line endings they were opened with."));
        p.append(&heading("Encoding"));
        let encs = Encoding::all();
        let names: Vec<String> = encs.iter().map(|e| e.label()).collect();
        let selected = encs.iter().position(|e| e.name() == self.settings.borrow().default_encoding).unwrap_or(0) as u32;
        self.pref_dropdown(&p, "Default encoding", &names, selected, move |s, i| s.default_encoding = encs[i as usize].name().to_string());
        self.pref_check(&p, "Write a byte order mark for UTF-8", |s| s.default_utf8_bom, |s, v| s.default_utf8_bom = v);
        p.append(&heading("Line endings"));
        let current = self.settings.borrow().default_line_ending;
        self.pref_radio(&p, &[("LF (Unix, macOS)", LineEnding::Lf), ("CRLF (Windows)", LineEnding::Crlf), ("CR (classic Mac)", LineEnding::Cr)], current, |s, v| s.default_line_ending = v);
        self.file_types_section(&p);
        p
    }

    /// One switch per family of file types: on makes Notey the system default for those types,
    /// off resets them to the system's own choice. The system is the source of truth, so nothing
    /// is stored in the settings.
    fn file_types_section(self: &Rc<Self>, p: &GtkBox) {
        p.append(&heading("Default app"));
        let Some(app) = gio::DesktopAppInfo::new(APP_DESKTOP_ID) else {
            p.append(&note("Notey is not installed with a launcher, so it cannot be chosen as a default app. Install the package, or run scripts/dev-install.sh when running from the source tree."));
            return;
        };
        p.append(&note("Open these kinds of files in Notey when you double-click them in the file manager."));
        let app: gio::AppInfo = app.upcast();
        for (name, mimes) in FILE_TYPE_GROUPS {
            let row = GtkBox::new(Orientation::Horizontal, 12);
            row.append(&row_label(name));
            let switch = gtk::Switch::new();
            switch.set_valign(gtk::Align::Center);
            switch.set_tooltip_text(Some(&mimes.join("\n")));
            let is_default = |m: &&str| gio::AppInfo::default_for_type(m, false).is_some_and(|d| d.id().as_deref() == Some(APP_DESKTOP_ID));
            switch.set_active(mimes.iter().all(is_default));
            let weak = Rc::downgrade(self);
            let app = app.clone();
            switch.connect_state_set(move |sw, on| {
                let result = mimes.iter().try_for_each(|m| {
                    if on {
                        app.set_as_default_for_type(m)
                    } else if gio::AppInfo::default_for_type(m, false).is_some_and(|d| d.id().as_deref() == Some(APP_DESKTOP_ID)) {
                        gio::AppInfo::reset_type_associations(m);
                        Ok(())
                    } else {
                        Ok(())
                    }
                });
                if let (Err(e), Some(t)) = (&result, weak.upgrade()) {
                    t.toast(&format!("Could not change the default app: {e}"));
                }
                sw.set_state(on && result.is_ok());
                glib::Propagation::Stop
            });
            row.append(&switch);
            p.append(&row);
        }
    }

    fn privacy_page(self: &Rc<Self>) -> GtkBox {
        let p = page("Search & privacy");
        let weak = Rc::downgrade(self);
        self.pref_check(&p, "Remember recent searches", |s| s.remember_search_history, |s, v| s.remember_search_history = v);
        p.append(&Self::button("Clear search history", move || {
            if let Some(t) = weak.upgrade() {
                let mut s = t.settings.borrow_mut();
                s.search_history.clear();
                s.replace_history.clear();
                drop(s);
                t.save_settings();
            }
        }));
        p.append(&heading("Saved session"));
        p.append(&note("The tabs and unsaved text kept for the next launch. Clearing it does not close or change the tabs open now."));
        let weak = Rc::downgrade(self);
        p.append(&Self::button("Clear saved session", move || {
            if let Some(t) = weak.upgrade() {
                t.clear_saved_session();
                t.toast("Saved session cleared");
            }
        }));
        p
    }

    fn about_page(self: &Rc<Self>) -> GtkBox {
        let p = GtkBox::new(Orientation::Vertical, 12);
        p.set_margin_top(18);
        let identity = GtkBox::new(Orientation::Horizontal, 14);
        let icon = gtk::Image::from_icon_name("com.zorin.Notey");
        icon.set_pixel_size(64);
        icon.set_valign(gtk::Align::Start);
        identity.append(&icon);
        let text = GtkBox::new(Orientation::Vertical, 4);
        let name = Label::new(Some("NoteyMcNoteface"));
        name.add_css_class("title-2");
        name.set_xalign(0.0);
        text.append(&name);
        let version = Label::new(Some(&format!("Version {}", env!("CARGO_PKG_VERSION"))));
        version.add_css_class("dim-label");
        version.set_xalign(0.0);
        text.append(&version);
        identity.append(&text);
        p.append(&identity);
        let description = Label::new(Some("A tabbed text editor for Linux that remembers your notes between sessions, designed for Zorin OS and GNOME."));
        description.set_wrap(true);
        description.set_xalign(0.0);
        description.set_margin_top(8);
        p.append(&description);
        p.append(&heading("Product information"));
        for line in [
            "Application ID: com.zorin.Notey".to_string(),
            format!("License: {}", env!("CARGO_PKG_LICENSE")),
            "Built with GTK 4, libadwaita and GtkSourceView 5".to_string(),
        ] {
            let l = Label::new(Some(&line));
            l.set_xalign(0.0);
            l.set_selectable(true);
            p.append(&l);
        }
        let weak = Rc::downgrade(self);
        let shortcuts = Self::button("Keyboard shortcuts", move || {
            if let Some(t) = weak.upgrade() {
                t.show_shortcuts();
            }
        });
        shortcuts.set_margin_top(12);
        p.append(&shortcuts);
        p
    }

    // ----------------------------------------------------------- quick actions

    fn quick_actions_page(self: &Rc<Self>) -> GtkBox {
        let p = page("Quick actions");
        p.append(&note("The icons under the title bar. Move actions with the arrows, take them off with the minus button, add them back from the list below, and use separators to divide the bar into groups."));
        self.pref_check(&p, "Show the quick actions bar", |s| s.show_toolbar, |s, v| s.show_toolbar = v);
        let list = GtkBox::new(Orientation::Vertical, 6);
        list.set_margin_top(8);
        self.fill_quick(&list);
        p.append(&list);
        let (weak, list2) = (Rc::downgrade(self), list.clone());
        let reset = Self::button("Restore the default set", move || {
            if let Some(t) = weak.upgrade() {
                t.settings.borrow_mut().quick_actions = super::menus::default_quick_actions();
                t.apply_settings_everywhere();
                t.fill_quick(&list2);
            }
        });
        reset.set_margin_top(12);
        p.append(&reset);
        p
    }

    /// Drag the handle to move row `i`; every row accepts a drop and takes the dragged row's place.
    fn make_draggable(self: &Rc<Self>, row: &GtkBox, handle: &gtk::Image, i: usize, list: &GtkBox) {
        let source = gtk::DragSource::new();
        source.set_actions(gtk::gdk::DragAction::MOVE);
        source.connect_prepare(move |_, _, _| Some(gtk::gdk::ContentProvider::for_value(&(i as u32).to_value())));
        let r = row.clone();
        source.connect_drag_begin(move |s, _| {
            r.add_css_class("quick-dragging");
            s.set_icon(Some(&gtk::WidgetPaintable::new(Some(&r))), 0, 0);
        });
        let r = row.clone();
        source.connect_drag_end(move |_, _, _| r.remove_css_class("quick-dragging"));
        handle.add_controller(source);

        let target = gtk::DropTarget::new(glib::Type::U32, gtk::gdk::DragAction::MOVE);
        let r = row.clone();
        target.connect_enter(move |_, _, _| {
            r.add_css_class("quick-drop");
            gtk::gdk::DragAction::MOVE
        });
        let r = row.clone();
        target.connect_leave(move |_| r.remove_css_class("quick-drop"));
        let (weak, list) = (Rc::downgrade(self), list.clone());
        target.connect_drop(move |t, value, _, _| {
            let (Some(win), Ok(from)) = (weak.upgrade(), value.get::<u32>()) else { return false };
            t.widget().remove_css_class("quick-drop");
            move_entry(&mut win.settings.borrow_mut().quick_actions, from as usize, i);
            win.apply_settings_everywhere();
            // Rebuilding inside the drop handler would destroy the widget mid-signal.
            let list = list.clone();
            glib::idle_add_local_once(move || win.fill_quick(&list));
            true
        });
        row.add_controller(target);
    }

    /// Rows for what is on the bar, in order (separators included), then the actions that are off.
    /// Bar entries are addressed by position because separators repeat.
    fn fill_quick(self: &Rc<Self>, list: &GtkBox) {
        while let Some(c) = list.first_child() {
            list.remove(&c);
        }
        let on_bar: Vec<String> = self.settings.borrow().quick_actions.iter().filter(|id| *id == SEPARATOR || quick_action(id).is_some()).cloned().collect();
        let off: Vec<&str> = QUICK_ACTIONS.iter().map(|a| a.id).filter(|id| !on_bar.iter().any(|e| e == id)).collect();
        let edit = |f: QuickEdit| {
            let (weak, list) = (Rc::downgrade(self), list.clone());
            move || {
                if let Some(t) = weak.upgrade() {
                    f(&mut t.settings.borrow_mut().quick_actions);
                    t.apply_settings_everywhere();
                    t.fill_quick(&list);
                }
            }
        };
        let small = |icon: &str, tip: &str| {
            let b = Button::from_icon_name(icon);
            b.add_css_class("flat");
            b.set_tooltip_text(Some(tip));
            b
        };

        for (i, id) in on_bar.iter().enumerate() {
            let is_sep = id == SEPARATOR;
            let row = GtkBox::new(Orientation::Horizontal, 4);
            let handle = gtk::Image::from_icon_name("list-drag-handle-symbolic");
            handle.set_tooltip_text(Some("Drag to reorder"));
            handle.add_css_class("dim-label");
            handle.set_cursor_from_name(Some("grab"));
            row.append(&handle);
            self.make_draggable(&row, &handle, i, list);
            row.append(&row_label(&if is_sep { "── Separator ──".to_string() } else { quick_action(id).map_or_else(String::new, |a| a.tip.to_string()) }));
            for (icon, tip, delta) in [("go-up-symbolic", "Move up", -1i32), ("go-down-symbolic", "Move down", 1)] {
                let b = small(icon, tip);
                b.set_sensitive(if delta < 0 { i > 0 } else { i + 1 < on_bar.len() });
                let go = edit(Box::new(move |l| {
                    if let Some(p) = nth_known(l, i) {
                        let q = (p as i32 + delta).clamp(0, l.len() as i32 - 1) as usize;
                        l.swap(p, q);
                    }
                }));
                b.connect_clicked(move |_| go());
                row.append(&b);
            }
            let remove = small(if is_sep { "user-trash-symbolic" } else { "list-remove-symbolic" }, if is_sep { "Remove separator" } else { "Take off the bar" });
            let go = edit(Box::new(move |l| {
                if let Some(p) = nth_known(l, i) {
                    l.remove(p);
                }
            }));
            remove.connect_clicked(move |_| go());
            row.append(&remove);
            list.append(&row);
        }

        let add_sep = Self::button("Add a separator", {
            let go = edit(Box::new(|l| l.push(SEPARATOR.to_string())));
            move || go()
        });
        add_sep.set_margin_top(6);
        list.append(&add_sep);

        if !off.is_empty() {
            list.append(&heading("Not on the bar"));
        }
        for id in off {
            let Some(a) = quick_action(id) else { continue };
            let row = GtkBox::new(Orientation::Horizontal, 4);
            row.append(&row_label(a.tip));
            let add = small("list-add-symbolic", "Add to the bar");
            let id = id.to_string();
            let go = edit(Box::new(move |l| l.push(id.clone())));
            add.connect_clicked(move |_| go());
            row.append(&add);
            list.append(&row);
        }
    }

    pub fn show_shortcuts(self: &Rc<Self>) {
        let win = adw::Window::builder().transient_for(&self.window).modal(true).default_width(520).default_height(620).title("Keyboard Shortcuts").build();
        let page = adw::PreferencesPage::new();
        let mut group: Option<(String, adw::PreferencesGroup)> = None;
        for (g, title, _, keys) in super::SHORTCUTS {
            if group.as_ref().map(|(n, _)| n.as_str()) != Some(*g) {
                if let Some((_, done)) = group.take() {
                    page.add(&done);
                }
                group = Some((g.to_string(), adw::PreferencesGroup::builder().title(*g).build()));
            }
            let row = adw::ActionRow::builder().title(*title).build();
            let label = keys.iter().map(|k| accel_label(k)).collect::<Vec<_>>().join("  /  ");
            let l = gtk::Label::new(Some(&label));
            l.add_css_class("dim-label");
            row.add_suffix(&l);
            if let Some((_, g)) = &group {
                g.add(&row);
            }
        }
        if let Some((_, done)) = group {
            page.add(&done);
        }
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&adw::HeaderBar::new());
        toolbar.set_content(Some(&page));
        win.set_content(Some(&toolbar));
        win.present();
    }

    pub fn show_about(self: &Rc<Self>) {
        let about = adw::AboutWindow::builder()
            .transient_for(&self.window)
            .application_name("NoteyMcNoteface")
            .application_icon("com.zorin.Notey")
            .version(env!("CARGO_PKG_VERSION"))
            .comments("A tabbed text editor that remembers your notes between sessions.")
            .license_type(gtk::License::Gpl30)
            .build();
        about.present();
    }
}
