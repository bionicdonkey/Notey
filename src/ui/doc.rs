//! One open document: its text buffer, view and the metadata that is not text.
use crate::application::file_io::SizeTier;
use crate::core::encoding::Encoding;
use crate::core::line_ending::LineEnding;
use crate::core::session::{Cursor, DiskState, Selection, TabRecord};
use crate::core::text_ops::IndentStyle;
use crate::settings::Settings;
use gtk::prelude::*;
use sourceview5::prelude::*;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

pub struct DocState {
    pub path: Option<PathBuf>,
    pub untitled_number: u32,
    pub encoding: Encoding,
    pub bom: bool,
    pub line_ending: LineEnding,
    pub mixed_endings: bool,
    /// The encoding was guessed rather than known.
    pub guessed_encoding: bool,
    pub had_errors: bool,
    pub language_override: Option<String>,
    pub disk: Option<DiskState>,
    pub read_only: bool,
    pub tier: SizeTier,
    pub indent: Option<IndentStyle>,
}

pub struct Doc {
    pub id: String,
    pub buffer: sourceview5::Buffer,
    pub view: sourceview5::View,
    pub map: sourceview5::Map,
    /// The tab page's child: info bar, find bar slot and the editor.
    pub root: gtk::Box,
    pub info: gtk::Box,
    pub state: RefCell<DocState>,
    /// Bumped on every edit; the session snapshot only reads the text when this moved.
    pub generation: Cell<u64>,
    pub snapshot_generation: Cell<u64>,
    /// A `buffers/<id>.txt` file currently holds this tab's unsaved text.
    pub has_buffer_file: Cell<bool>,
    pub snapshot_timer: RefCell<Option<glib::SourceId>>,
    pub force_close: Cell<bool>,
    pub monitor: RefCell<Option<gio::FileMonitor>>,
    pub search_settings: sourceview5::SearchSettings,
    pub search: sourceview5::SearchContext,
    /// Line to scroll to on first map, once the view has a size.
    pub pending_scroll: Cell<Option<u32>>,
    /// An external-change banner is already showing.
    pub external_pending: Cell<bool>,
}

impl Doc {
    pub fn new(settings: &Settings, id: Option<String>) -> Self {
        let buffer = sourceview5::Buffer::new(None::<&gtk::TextTagTable>);
        buffer.set_highlight_matching_brackets(true);
        let view = sourceview5::View::with_buffer(&buffer);
        view.add_css_class("notey-editor");
        view.set_monospace(true);
        view.set_top_margin(6);
        view.set_bottom_margin(6);
        view.set_left_margin(8);
        view.set_right_margin(8);
        view.set_smart_home_end(sourceview5::SmartHomeEndType::Before);
        view.set_indent_on_tab(true);
        view.set_hexpand(true);
        view.set_vexpand(true);

        let scroller = gtk::ScrolledWindow::builder().child(&view).hexpand(true).vexpand(true).build();
        let map = sourceview5::Map::new();
        map.set_view(&view);
        map.set_visible(settings.show_minimap);
        let editor_row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        editor_row.append(&scroller);
        editor_row.append(&map);

        let info = gtk::Box::new(gtk::Orientation::Vertical, 0);
        info.set_visible(false);
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.append(&info);
        root.append(&editor_row);

        let search_settings = sourceview5::SearchSettings::new();
        search_settings.set_wrap_around(true);
        let search = sourceview5::SearchContext::new(&buffer, Some(&search_settings));
        search.set_highlight(false);
        let doc = Doc {
            id: id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            buffer,
            view,
            map,
            root,
            info,
            state: RefCell::new(DocState {
                path: None,
                untitled_number: 1,
                encoding: Encoding::from_name(&settings.default_encoding).unwrap_or(Encoding::Utf8),
                bom: settings.default_utf8_bom,
                line_ending: settings.default_line_ending,
                mixed_endings: false,
                guessed_encoding: false,
                had_errors: false,
                language_override: None,
                disk: None,
                read_only: false,
                tier: SizeTier::Normal,
                indent: None,
            }),
            generation: Cell::new(0),
            snapshot_generation: Cell::new(0),
            has_buffer_file: Cell::new(false),
            snapshot_timer: RefCell::new(None),
            force_close: Cell::new(false),
            monitor: RefCell::new(None),
            search,
            search_settings,
            pending_scroll: Cell::new(None),
            external_pending: Cell::new(false),
        };
        doc.apply_settings(settings);
        doc
    }

    pub fn apply_settings(&self, s: &Settings) {
        let indent = self.state.borrow().indent;
        let (spaces, width) = match indent {
            Some(IndentStyle::Tabs) => (false, s.indent_width),
            Some(IndentStyle::Spaces(n)) => (true, n as u32),
            None => (s.insert_spaces, s.indent_width),
        };
        let v = &self.view;
        v.set_show_line_numbers(s.line_numbers);
        let large = self.state.borrow().tier != SizeTier::Normal;
        v.set_highlight_current_line(s.highlight_current_line && !large);
        v.set_tab_width(width);
        v.set_indent_width(width as i32);
        v.set_insert_spaces_instead_of_tabs(spaces);
        v.set_auto_indent(s.auto_indent);
        v.set_wrap_mode(if s.word_wrap { gtk::WrapMode::WordChar } else { gtk::WrapMode::None });
        v.set_show_right_margin(s.right_margin > 0);
        if s.right_margin > 0 {
            v.set_right_margin_position(s.right_margin);
        }
        let drawer = v.space_drawer();
        drawer.set_enable_matrix(s.show_whitespace);
        if s.show_whitespace {
            drawer.set_types_for_locations(
                sourceview5::SpaceLocationFlags::ALL,
                sourceview5::SpaceTypeFlags::SPACE | sourceview5::SpaceTypeFlags::TAB,
            );
        }
        self.map.set_visible(s.show_minimap && !large);
        self.buffer.set_highlight_syntax(!large);
    }

    /// Replaces the whole text without touching undo history, then sets the modified flag.
    pub fn set_text(&self, text: &str, modified: bool) {
        self.buffer.begin_irreversible_action();
        self.buffer.set_text(text);
        self.buffer.end_irreversible_action();
        self.buffer.set_modified(modified);
        let start = self.buffer.start_iter();
        self.buffer.place_cursor(&start);
    }

    pub fn text(&self) -> String {
        let (s, e) = self.buffer.bounds();
        self.buffer.text(&s, &e, true).to_string()
    }

    pub fn is_dirty(&self) -> bool {
        self.buffer.is_modified()
    }

    pub fn path(&self) -> Option<PathBuf> {
        self.state.borrow().path.clone()
    }

    pub fn set_path(&self, path: Option<&Path>) {
        self.state.borrow_mut().path = path.map(Path::to_path_buf);
    }

    /// File name, or `Untitled N`.
    pub fn name(&self) -> String {
        let st = self.state.borrow();
        match &st.path {
            Some(p) => p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned()),
            None if st.untitled_number <= 1 => "Untitled".into(),
            None => format!("Untitled {}", st.untitled_number),
        }
    }

    pub fn cursor(&self) -> Cursor {
        let it = self.buffer.iter_at_offset(self.buffer.cursor_position());
        Cursor { line: it.line() as u32, column: it.line_offset() as u32 }
    }

    pub fn selection(&self) -> Option<Selection> {
        let (s, e) = self.buffer.selection_bounds()?;
        Some(Selection {
            start: Cursor { line: s.line() as u32, column: s.line_offset() as u32 },
            end: Cursor { line: e.line() as u32, column: e.line_offset() as u32 },
        })
    }

    fn iter_at(&self, c: Cursor) -> gtk::TextIter {
        let last = (self.buffer.line_count() - 1).max(0);
        let line = (c.line as i32).min(last);
        let mut it = self.buffer.iter_at_line(line).unwrap_or_else(|| self.buffer.end_iter());
        let chars = it.chars_in_line();
        // `chars_in_line` includes the break, so a line offset may land on it at most.
        it.set_line_offset((c.column as i32).min((chars - 1).max(0)));
        it
    }

    pub fn set_cursor(&self, c: Cursor) {
        let it = self.iter_at(c);
        self.buffer.place_cursor(&it);
    }

    pub fn set_selection(&self, s: Selection) {
        let (a, b) = (self.iter_at(s.start), self.iter_at(s.end));
        self.buffer.select_range(&a, &b);
    }

    pub fn top_visible_line(&self) -> u32 {
        let rect = self.view.visible_rect();
        let (it, _) = self.view.line_at_y(rect.y());
        it.line().max(0) as u32
    }

    /// Scrolls so `line` is at the top. Needs an allocated view, so callers use an idle callback.
    pub fn scroll_to_line_top(&self, line: u32) {
        let it = self.iter_at(Cursor { line, column: 0 });
        self.view.scroll_to_iter(&mut it.clone(), 0.0, true, 0.0, 0.0);
    }

    pub fn center_on_cursor(&self) {
        let mut it = self.buffer.iter_at_offset(self.buffer.cursor_position());
        self.view.scroll_to_iter(&mut it, 0.0, true, 0.0, 0.5);
    }

    /// Applies the best language for the file name and content; honours the manual override.
    pub fn detect_language(&self) {
        let manager = sourceview5::LanguageManager::default();
        let st = self.state.borrow();
        let lang = if let Some(id) = &st.language_override {
            manager.language(id)
        } else {
            let name = st.path.as_ref().map(|p| p.to_path_buf());
            let mut found = name.as_deref().and_then(|n| manager.guess_language(Some(n), None));
            if found.is_none() {
                let text = self.text();
                let head: String = text.lines().take(5).collect::<Vec<_>>().join("\n");
                let tail: Vec<&str> = text.lines().rev().take(5).collect();
                let tail = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
                let file_name = st.path.as_ref().and_then(|p| p.file_name()).and_then(|n| n.to_str());
                found = crate::core::language::fallback_language(file_name, &head, &tail)
                    .and_then(|id| manager.language(id));
            }
            found
        };
        self.buffer.set_language(lang.as_ref());
    }

    pub fn language_id(&self) -> Option<String> {
        self.buffer.language().map(|l| l.id().to_string())
    }

    pub fn language_name(&self) -> String {
        self.buffer.language().map_or_else(|| "Plain Text".into(), |l| l.name().to_string())
    }

    /// Metadata for the manifest. `has_buffer` says whether the unsaved text is stored.
    pub fn record(&self, title: &str, pinned: bool) -> TabRecord {
        let st = self.state.borrow();
        let mut r = TabRecord::new_untitled(title);
        r.id = self.id.clone();
        r.path = st.path.as_ref().map(|p| p.to_string_lossy().into_owned());
        r.pinned = pinned;
        r.dirty = self.is_dirty();
        r.buffer_file = self.has_buffer_file.get().then(|| TabRecord::buffer_file_name(&self.id));
        r.base = st.disk.clone();
        r.encoding = st.encoding.name().into();
        r.bom = st.bom;
        r.line_ending = st.line_ending;
        r.language = st.language_override.clone();
        r.cursor = self.cursor();
        r.scroll_line = self.top_visible_line();
        r.selection = self.selection();
        r
    }

    /// Whether the unsaved text must be stored in the session.
    pub fn needs_buffer_file(&self) -> bool {
        if self.state.borrow().path.is_none() {
            self.buffer.char_count() > 0
        } else {
            self.is_dirty()
        }
    }
}
