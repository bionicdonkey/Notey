//! Find and replace: plain text and regular expressions, in this tab, the selection or all tabs.
use super::dialogs::{ask, Tone};
use super::doc::Doc;
use super::window::Notey;
use crate::core::search::{regex_source, translate_replacement, Query};
use adw::prelude::*;
use sourceview5::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

const MAX_HITS: usize = 500;
const HISTORY_LEN: usize = 20;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Tab,
    Selection,
    AllTabs,
}

struct Hit {
    doc_id: String,
    start: i32,
    end: i32,
}

pub struct FindBar {
    pub root: gtk::Box,
    pub find_entry: gtk::Entry,
    pub replace_entry: gtk::Entry,
    replace_row: gtk::Box,
    pub count: gtk::Label,
    pub error: gtk::Label,
    pub case: gtk::ToggleButton,
    pub word: gtk::ToggleButton,
    pub regex: gtk::ToggleButton,
    pub scope: gtk::DropDown,
    pub results: gtk::ListBox,
    pub results_scroller: gtk::ScrolledWindow,
    hits: RefCell<Vec<Hit>>,
    history_index: Cell<Option<usize>>,
    debounce: RefCell<Option<glib::SourceId>>,
    /// The range captured when "Selection" scope was chosen.
    selection_scope: Cell<Option<(i32, i32)>>,
    pub visible: Cell<bool>,
    /// A re-check of an unfinished match count is already scheduled.
    recount_pending: Cell<bool>,
}

impl FindBar {
    pub fn new() -> Self {
        let find_entry = gtk::Entry::builder().placeholder_text("Find").hexpand(true).build();
        find_entry.set_primary_icon_name(Some("edit-find-symbolic"));
        let replace_entry = gtk::Entry::builder().placeholder_text("Replace with").hexpand(true).build();
        let count = gtk::Label::new(None);
        count.add_css_class("dim-label");
        count.set_width_chars(12);
        let error = gtk::Label::new(None);
        error.add_css_class("error");
        error.set_xalign(0.0);
        error.set_visible(false);
        error.set_wrap(true);

        let toggle = |label: &str, tip: &str| {
            let b = gtk::ToggleButton::with_label(label);
            b.set_tooltip_text(Some(tip));
            b
        };
        let case = toggle("Aa", "Match case");
        let word = toggle("ab", "Match whole word");
        let regex = toggle(".*", "Regular expression (PCRE2: \\1 groups, lookarounds; ^ and $ match at line ends)");
        let scope = gtk::DropDown::from_strings(&["This tab", "Selection", "All tabs"]);
        scope.set_tooltip_text(Some("Where to search"));

        let prev = gtk::Button::from_icon_name("go-up-symbolic");
        prev.set_tooltip_text(Some("Previous match (Shift+F3)"));
        prev.set_action_name(Some("win.find-prev"));
        let next = gtk::Button::from_icon_name("go-down-symbolic");
        next.set_tooltip_text(Some("Next match (F3)"));
        next.set_action_name(Some("win.find-next"));
        let close = gtk::Button::from_icon_name("window-close-symbolic");
        close.set_has_frame(false);
        close.set_tooltip_text(Some("Close (Esc)"));
        close.set_action_name(Some("win.find-toggle"));

        let row1 = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        for w in [find_entry.upcast_ref::<gtk::Widget>(), prev.upcast_ref(), next.upcast_ref(), close.upcast_ref()] {
            row1.append(w);
        }
        let options = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        count.set_hexpand(true);
        count.set_xalign(0.0);
        for w in [count.upcast_ref::<gtk::Widget>(), case.upcast_ref(), word.upcast_ref(), regex.upcast_ref(), scope.upcast_ref()] {
            options.append(w);
        }
        let replace_btn = gtk::Button::with_label("Replace");
        replace_btn.set_action_name(Some("win.replace-one"));
        let replace_all = gtk::Button::with_label("Replace All");
        replace_all.set_action_name(Some("win.replace-all"));
        let replace_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        replace_row.append(&replace_entry);
        replace_row.append(&replace_btn);
        replace_row.append(&replace_all);
        replace_row.set_visible(false);

        let results = gtk::ListBox::new();
        results.add_css_class("notey-results");
        results.set_selection_mode(gtk::SelectionMode::Single);
        let results_scroller = gtk::ScrolledWindow::builder().child(&results).max_content_height(180).propagate_natural_height(true).build();
        results_scroller.set_visible(false);

        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        root.add_css_class("notey-findbar");
        root.append(&row1);
        root.append(&replace_row);
        root.append(&options);
        root.append(&error);
        root.append(&results_scroller);
        root.set_visible(false);

        Self {
            root,
            find_entry,
            replace_entry,
            replace_row,
            count,
            error,
            case,
            word,
            regex,
            scope,
            results,
            results_scroller,
            hits: RefCell::new(Vec::new()),
            history_index: Cell::new(None),
            debounce: RefCell::new(None),
            selection_scope: Cell::new(None),
            visible: Cell::new(false),
            recount_pending: Cell::new(false),
        }
    }

    pub fn scope(&self) -> Scope {
        match self.scope.selected() {
            1 => Scope::Selection,
            2 => Scope::AllTabs,
            _ => Scope::Tab,
        }
    }

    pub fn query(&self) -> Query {
        Query {
            pattern: self.find_entry.text().to_string(),
            regex: self.regex.is_active(),
            case_sensitive: self.case.is_active(),
            whole_word: self.word.is_active(),
        }
    }
}

impl Default for FindBar {
    fn default() -> Self {
        Self::new()
    }
}

/// Pushes the query into a document's GtkSourceView search settings.
fn configure(doc: &Doc, q: &Query) {
    let s = &doc.search_settings;
    s.set_case_sensitive(q.case_sensitive);
    s.set_regex_enabled(q.regex);
    // Whole word on a regex is expressed in the pattern; on plain text GtkSourceView does it.
    s.set_at_word_boundaries(q.whole_word && !q.regex);
    let text = if q.regex && q.whole_word { regex_source(q) } else { q.pattern.clone() };
    s.set_search_text(if text.is_empty() { None } else { Some(&text) });
}

/// `SearchContext::replace_all` in the bindings asserts that a FALSE return carries an error, but
/// GtkSourceView also returns FALSE for "nothing to replace" (invalid or empty pattern). Call the
/// C function directly and look only at the error.
fn replace_all_in(ctx: &sourceview5::SearchContext, replacement: &str) -> Result<(), glib::Error> {
    use glib::translate::*;
    unsafe {
        let mut error = std::ptr::null_mut();
        let _ = sourceview5::ffi::gtk_source_search_context_replace_all(ctx.to_glib_none().0, replacement.to_glib_none().0, replacement.len() as i32, &mut error);
        if error.is_null() {
            Ok(())
        } else {
            Err(from_glib_full(error))
        }
    }
}

fn compile(q: &Query) -> Result<Option<glib::Regex>, glib::Error> {
    let mut flags = glib::RegexCompileFlags::MULTILINE;
    if !q.case_sensitive {
        flags |= glib::RegexCompileFlags::CASELESS;
    }
    glib::Regex::new(&regex_source(q), flags, glib::RegexMatchFlags::DEFAULT)
}

/// Walks every match of `re` in `text`, passing the match and its character offsets to `visit`.
/// Stops after `limit`. Zero-length matches (`^`, `$`, lookarounds) are included.
fn walk_matches(text: &str, re: &glib::Regex, limit: usize, mut visit: impl FnMut(&glib::MatchInfo, i32, i32)) {
    // Byte offset -> char offset, advancing monotonically.
    let (mut last_byte, mut last_chars) = (0usize, 0i32);
    let mut to_chars = |byte: usize| {
        last_chars += text[last_byte..byte].chars().count() as i32;
        last_byte = byte;
        last_chars
    };
    let gtext = glib::GString::from(text);
    // `MatchInfo::next` trips a debug assertion in the bindings at the last match, so step manually.
    let (mut pos, mut count) = (0usize, 0usize);
    while count < limit && pos <= text.len() {
        let Ok(info) = re.match_full(gtext.as_gstr(), pos as i32, glib::RegexMatchFlags::DEFAULT) else { break };
        if !info.matches() {
            break;
        }
        let Some((s, e)) = info.fetch_pos(0) else { break };
        let (s, e) = (s as usize, e as usize);
        visit(&info, to_chars(s), to_chars(e));
        count += 1;
        pos = if e > s {
            e
        } else {
            // Empty match: step over one character so the scan always advances.
            e + text[e..].chars().next().map_or(1, char::len_utf8)
        };
    }
}

/// All matches in `text` as character offsets.
fn scan(text: &str, re: &glib::Regex, limit: usize) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    walk_matches(text, re, limit, |_, s, e| out.push((s, e)));
    out
}

/// Every match with its replacement text (`\1`-style references expanded).
fn scan_replacements(text: &str, re: &glib::Regex, replacement: &str) -> Vec<(i32, i32, String)> {
    let mut out = Vec::new();
    walk_matches(text, re, usize::MAX, |info, s, e| {
        let expanded = info.expand_references(replacement).ok().flatten().map_or_else(|| replacement.to_string(), |g| g.to_string());
        out.push((s, e, expanded));
    });
    out
}

/// Replaces every regex match in one undo step, last to first so earlier offsets stay valid.
/// GtkSourceView's own replace-all skips zero-length matches, which `^` and `$` rely on.
fn replace_regex_matches(doc: &Doc, re: &glib::Regex, replacement: &str) -> usize {
    let hits = scan_replacements(&doc.text(), re, replacement);
    let buf = &doc.buffer;
    buf.begin_user_action();
    for (s, e, text) in hits.iter().rev() {
        buf.delete(&mut buf.iter_at_offset(*s), &mut buf.iter_at_offset(*e));
        buf.insert(&mut buf.iter_at_offset(*s), text);
    }
    buf.end_user_action();
    hits.len()
}

impl Notey {
    pub fn install_find(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        let f = &self.find;
        let s = self.settings.borrow().clone();
        f.case.set_active(s.search_case_sensitive);
        f.word.set_active(s.search_whole_word);
        f.regex.set_active(s.search_regex);

        let changed = {
            let weak = weak.clone();
            move || {
                if let Some(t) = weak.upgrade() {
                    t.query_changed();
                }
            }
        };
        f.find_entry.connect_changed({
            let changed = changed.clone();
            move |_| changed()
        });
        for b in [&f.case, &f.word, &f.regex] {
            let changed = changed.clone();
            b.connect_toggled(move |_| changed());
        }
        f.scope.connect_selected_notify({
            let weak = weak.clone();
            move |_| {
                if let Some(t) = weak.upgrade() {
                    t.scope_changed();
                }
            }
        });
        f.find_entry.connect_activate({
            let weak = weak.clone();
            move |_| {
                if let Some(t) = weak.upgrade() {
                    t.remember_query();
                    t.find_step(true);
                }
            }
        });
        f.replace_entry.connect_activate({
            let weak = weak.clone();
            move |_| {
                if let Some(t) = weak.upgrade() {
                    t.replace_one();
                }
            }
        });
        // Esc closes; Shift+Enter goes back; Up/Down walk the history.
        let key = gtk::EventControllerKey::new();
        key.connect_key_pressed({
            let weak = weak.clone();
            move |_, keyval, _, state| {
                use gtk::gdk::Key;
                let Some(t) = weak.upgrade() else { return glib::Propagation::Proceed };
                match keyval {
                    Key::Escape => {
                        t.close_find();
                        glib::Propagation::Stop
                    }
                    Key::Return | Key::KP_Enter if state.contains(gtk::gdk::ModifierType::SHIFT_MASK) => {
                        t.find_step(false);
                        glib::Propagation::Stop
                    }
                    Key::Up => {
                        t.history_step(-1);
                        glib::Propagation::Stop
                    }
                    Key::Down => {
                        t.history_step(1);
                        glib::Propagation::Stop
                    }
                    _ => glib::Propagation::Proceed,
                }
            }
        });
        f.find_entry.add_controller(key);
        let key2 = gtk::EventControllerKey::new();
        key2.connect_key_pressed({
            let weak = weak.clone();
            move |_, keyval, _, _| {
                if keyval == gtk::gdk::Key::Escape {
                    if let Some(t) = weak.upgrade() {
                        t.close_find();
                        return glib::Propagation::Stop;
                    }
                }
                glib::Propagation::Proceed
            }
        });
        f.replace_entry.add_controller(key2);
        f.results.connect_row_activated({
            let weak = weak.clone();
            move |_, row| {
                if let Some(t) = weak.upgrade() {
                    t.jump_to_hit(row.index() as usize);
                }
            }
        });

        // Actions.
        let toggle = gio::SimpleAction::new_stateful("find-toggle", None, &false.to_variant());
        let w = weak.clone();
        toggle.connect_activate(move |_, _| {
            if let Some(t) = w.upgrade() {
                if t.find.visible.get() {
                    t.close_find();
                } else {
                    t.open_find(false);
                }
            }
        });
        self.window.add_action(&toggle);
        let simple = |name: &str, f: fn(&Rc<Notey>)| {
            let a = gio::SimpleAction::new(name, None);
            let w = weak.clone();
            a.connect_activate(move |_, _| {
                if let Some(t) = w.upgrade() {
                    f(&t);
                }
            });
            self.window.add_action(&a);
        };
        simple("find", |t| t.open_find(false));
        simple("find-replace", |t| t.open_find(true));
        simple("find-next", |t| t.find_step(true));
        simple("find-prev", |t| t.find_step(false));
        simple("replace-one", |t| t.replace_one());
        simple("replace-all", |t| t.replace_all());
    }

    /// Keeps the match counter in step with the asynchronous counting GtkSourceView does.
    pub fn on_doc_added_find(self: &Rc<Self>, doc: &Rc<Doc>) {
        let weak = Rc::downgrade(self);
        doc.search.connect_occurrences_count_notify(move |_| {
            if let Some(t) = weak.upgrade() {
                t.update_find_count();
            }
        });
    }

    fn set_find_toggle_state(&self, on: bool) {
        if let Some(a) = self.window.lookup_action("find-toggle").and_downcast::<gio::SimpleAction>() {
            a.set_state(&on.to_variant());
        }
    }

    pub fn open_find(self: &Rc<Self>, replace: bool) {
        let f = &self.find;
        f.visible.set(true);
        f.root.set_visible(true);
        f.replace_row.set_visible(replace || f.replace_row.is_visible());
        self.set_find_toggle_state(true);
        if let Some(doc) = self.current_doc() {
            if let Some((a, b)) = doc.buffer.selection_bounds() {
                if a.line() == b.line() && b.offset() - a.offset() < 200 {
                    f.find_entry.set_text(&doc.buffer.text(&a, &b, false));
                } else if f.scope() == Scope::Tab && a.line() != b.line() {
                    f.selection_scope.set(Some((a.offset(), b.offset())));
                }
            }
        }
        let target = if replace && !f.find_entry.text().is_empty() { &f.replace_entry } else { &f.find_entry };
        target.grab_focus();
        target.select_region(0, -1);
        self.query_changed();
    }

    pub fn close_find(self: &Rc<Self>) {
        let f = &self.find;
        f.visible.set(false);
        f.root.set_visible(false);
        self.set_find_toggle_state(false);
        for d in self.docs.borrow().iter() {
            d.search.set_highlight(false);
        }
        self.persist_find_options();
        if let Some(d) = self.current_doc() {
            d.view.grab_focus();
        }
    }

    fn persist_find_options(&self) {
        let q = self.find.query();
        let mut s = self.settings.borrow_mut();
        s.search_case_sensitive = q.case_sensitive;
        s.search_whole_word = q.whole_word;
        s.search_regex = q.regex;
    }

    /// Debounced re-run of the search after the query or options changed.
    fn query_changed(self: &Rc<Self>) {
        if let Some(id) = self.find.debounce.take() {
            id.remove();
        }
        let weak = Rc::downgrade(self);
        let id = glib::timeout_add_local_once(Duration::from_millis(100), move || {
            if let Some(t) = weak.upgrade() {
                *t.find.debounce.borrow_mut() = None;
                t.run_search();
            }
        });
        *self.find.debounce.borrow_mut() = Some(id);
    }

    fn scope_changed(self: &Rc<Self>) {
        if self.find.scope() == Scope::Selection {
            let range = self.current_doc().and_then(|d| d.buffer.selection_bounds()).map(|(a, b)| (a.offset(), b.offset()));
            self.find.selection_scope.set(range);
            if range.is_none() {
                self.toast("Select some text first to search within it");
            }
        }
        self.run_search();
    }

    /// Applies the query to the current tab (and refreshes the all-tabs list).
    pub fn run_search(self: &Rc<Self>) {
        let f = &self.find;
        if !f.visible.get() {
            return;
        }
        let q = f.query();
        f.error.set_visible(false);
        f.find_entry.remove_css_class("error");
        for d in self.docs.borrow().iter() {
            d.search.set_highlight(false);
        }
        let Some(doc) = self.current_doc() else { return };
        let big = doc.state.borrow().tier != crate::application::file_io::SizeTier::Normal;
        configure(&doc, &q);
        doc.search.set_highlight(!big);

        if q.regex && !q.pattern.is_empty() {
            if let Err(e) = compile(&q) {
                f.error.set_label(&format!("Invalid regular expression: {}", e.message()));
                f.error.set_visible(true);
                f.find_entry.add_css_class("error");
                f.count.set_label("");
                f.results_scroller.set_visible(false);
                return;
            }
        }
        f.results_scroller.set_visible(false);
        if f.scope() == Scope::AllTabs {
            self.fill_results(&q);
        }
        self.update_find_count();
    }

    pub fn update_find_count(&self) {
        let f = &self.find;
        if !f.visible.get() {
            return;
        }
        let Some(doc) = self.current_doc() else { return };
        if f.find_entry.text().is_empty() {
            f.count.set_label("");
            return;
        }
        if f.scope() == Scope::AllTabs {
            let n = f.hits.borrow().len();
            f.count.set_label(&match n {
                0 => "No results".to_string(),
                n if n >= MAX_HITS => format!("{MAX_HITS}+ in all tabs"),
                n => format!("{n} in all tabs"),
            });
            return;
        }
        if doc.state.borrow().tier != crate::application::file_io::SizeTier::Normal {
            f.count.set_label("");
            return;
        }
        let total = doc.search.occurrences_count();
        if total < 0 {
            f.count.set_label("Counting…");
            self.recheck_count_soon();
        } else if total == 0 {
            f.count.set_label("No results");
        } else {
            let pos = doc
                .buffer
                .selection_bounds()
                .map(|(a, b)| doc.search.occurrence_position(&a, &b))
                .filter(|p| *p > 0);
            f.count.set_label(&match pos {
                Some(p) => format!("{p} of {total}"),
                None => format!("{total} found"),
            });
        }
    }

    /// GtkSourceView counts matches in the background and says so with a property notification. If
    /// that notification is ever missed the label would stay on "Counting…" for good, so look again
    /// shortly. It stops by itself once the count is known or the find bar is closed.
    fn recheck_count_soon(&self) {
        if self.find.recount_pending.replace(true) {
            return;
        }
        let Some(this) = self.weak_self.borrow().upgrade() else {
            self.find.recount_pending.set(false);
            return;
        };
        glib::timeout_add_local_once(Duration::from_millis(150), move || {
            this.find.recount_pending.set(false);
            this.update_find_count();
        });
    }

    fn remember_query(&self) {
        if !self.settings.borrow().remember_search_history {
            return;
        }
        let text = self.find.find_entry.text().to_string();
        if text.is_empty() {
            return;
        }
        let mut s = self.settings.borrow_mut();
        s.search_history.retain(|h| *h != text);
        s.search_history.insert(0, text);
        s.search_history.truncate(HISTORY_LEN);
        self.find.history_index.set(None);
    }

    fn history_step(&self, dir: i32) {
        let hist = self.settings.borrow().search_history.clone();
        if hist.is_empty() {
            return;
        }
        let cur = self.find.history_index.get();
        let next = match (cur, dir) {
            (None, d) if d < 0 => Some(0),
            (None, _) => None,
            (Some(i), d) if d < 0 => Some((i + 1).min(hist.len() - 1)),
            (Some(0), _) => None,
            (Some(i), _) => Some(i - 1),
        };
        self.find.history_index.set(next);
        if let Some(i) = next {
            self.find.find_entry.set_text(&hist[i]);
            self.find.find_entry.set_position(-1);
        }
    }

    /// Selects the next or previous match in the current tab.
    pub fn find_step(self: &Rc<Self>, forward: bool) {
        if !self.find.visible.get() {
            self.open_find(false);
            return;
        }
        let Some(doc) = self.current_doc() else { return };
        let q = self.find.query();
        if q.pattern.is_empty() {
            return;
        }
        configure(&doc, &q);
        if self.find.scope() == Scope::AllTabs {
            self.step_all_tabs(forward);
            return;
        }
        let (this, range) = (self.clone(), (self.find.scope() == Scope::Selection).then(|| self.find.selection_scope.get()).flatten());
        glib::spawn_future_local(async move {
            let buf = &doc.buffer;
            let start = match buf.selection_bounds() {
                Some((a, b)) => if forward { b } else { a },
                None => buf.iter_at_offset(buf.cursor_position()),
            };
            let found = if forward { doc.search.forward_future(&start).await } else { doc.search.backward_future(&start).await };
            let Ok((s, e, wrapped)) = found else {
                this.find.count.set_label("No results");
                return;
            };
            if let Some((lo, hi)) = range {
                if s.offset() < lo || e.offset() > hi {
                    this.toast("No more matches in the selection");
                    return;
                }
            }
            buf.select_range(&e, &s);
            doc.view.scroll_to_mark(&buf.get_insert(), 0.1, true, 0.0, 0.5);
            this.update_find_count();
            if wrapped {
                this.toast("Wrapped around");
            }
        });
    }

    fn replacement(&self) -> String {
        let r = self.find.replace_entry.text().to_string();
        if self.find.regex.is_active() { translate_replacement(&r) } else { r }
    }

    pub fn replace_one(self: &Rc<Self>) {
        let Some(doc) = self.current_doc() else { return };
        if doc.state.borrow().read_only {
            return;
        }
        let q = self.find.query();
        if q.pattern.is_empty() {
            return;
        }
        configure(&doc, &q);
        let (this, rep) = (self.clone(), self.replacement());
        glib::spawn_future_local(async move {
            if let Some((a, b)) = doc.buffer.selection_bounds() {
                // Only replace when the selection is itself a match.
                if let Ok((ms, me, _)) = doc.search.forward_future(&a).await {
                    if ms.offset() == a.offset() && me.offset() == b.offset() {
                        let (mut ms, mut me) = (ms, me);
                        if let Err(e) = doc.search.replace(&mut ms, &mut me, &rep) {
                            this.toast(&format!("Replace failed: {}", e.message()));
                            return;
                        }
                    }
                }
            }
            this.find_step(true);
        });
    }

    pub fn replace_all(self: &Rc<Self>) {
        let q = self.find.query();
        if q.pattern.is_empty() {
            return;
        }
        let rep = self.replacement();
        match self.find.scope() {
            Scope::AllTabs => {
                let this = self.clone();
                glib::spawn_future_local(async move { this.replace_all_tabs(q, rep).await });
            }
            Scope::Selection => self.replace_in_selection(&q, &rep),
            Scope::Tab => {
                let Some(doc) = self.current_doc() else { return };
                if doc.state.borrow().read_only {
                    return;
                }
                configure(&doc, &q);
                let compiled = compile(&q).ok().flatten();
                if q.regex {
                    // GtkSourceView's replace-all skips zero-length matches; do it here.
                    let count = compiled.map_or(0, |re| replace_regex_matches(&doc, &re, &rep));
                    self.toast(&format!("Replaced {count}"));
                } else {
                    let count = compiled.map_or(0, |re| scan(&doc.text(), &re, usize::MAX).len());
                    match replace_all_in(&doc.search, &rep) {
                        Ok(()) => self.toast(&format!("Replaced {count}")),
                        Err(e) => self.toast(&format!("Replace failed: {}", e.message())),
                    }
                }
            }
        }
    }

    fn replace_in_selection(self: &Rc<Self>, q: &Query, rep: &str) {
        let Some(doc) = self.current_doc() else { return };
        let Some((lo, hi)) = self.find.selection_scope.get() else {
            self.toast("Select some text first to search within it");
            return;
        };
        let Ok(Some(re)) = compile(q) else { return };
        let text: String = doc.buffer.text(&doc.buffer.iter_at_offset(lo), &doc.buffer.iter_at_offset(hi), true).into();
        let count = scan(&text, &re, usize::MAX).len();
        let new = if q.regex {
            re.replace(&text, 0, rep, glib::RegexMatchFlags::DEFAULT)
        } else {
            re.replace_literal(&text, 0, rep, glib::RegexMatchFlags::DEFAULT)
        };
        if let Ok(new) = new {
            let new = new.to_string();
            let buf = &doc.buffer;
            buf.begin_user_action();
            buf.delete(&mut buf.iter_at_offset(lo), &mut buf.iter_at_offset(hi));
            buf.insert(&mut buf.iter_at_offset(lo), &new);
            buf.end_user_action();
            let end = lo + new.chars().count() as i32;
            self.find.selection_scope.set(Some((lo, end)));
            buf.select_range(&buf.iter_at_offset(lo), &buf.iter_at_offset(end));
            self.toast(&format!("Replaced {count}"));
        }
    }

    // ------------------------------------------------------------- all tabs

    fn fill_results(&self, q: &Query) {
        let f = &self.find;
        while let Some(c) = f.results.first_child() {
            f.results.remove(&c);
        }
        f.hits.borrow_mut().clear();
        if q.pattern.is_empty() {
            return;
        }
        let Ok(Some(re)) = compile(q) else { return };
        for doc in self.docs_in_order() {
            let text = doc.text();
            let room = MAX_HITS.saturating_sub(f.hits.borrow().len());
            for (s, e) in scan(&text, &re, room) {
                let it = doc.buffer.iter_at_offset(s);
                let line = it.line();
                let line_start = doc.buffer.iter_at_line(line).map_or(0, |i| i.offset());
                let mut line_end = doc.buffer.iter_at_line(line).unwrap_or_else(|| doc.buffer.end_iter());
                line_end.forward_to_line_end();
                let preview: String = doc.buffer.text(&doc.buffer.iter_at_offset(line_start), &line_end, false).trim().chars().take(140).collect();
                let label = gtk::Label::builder().xalign(0.0).ellipsize(gtk::pango::EllipsizeMode::End).build();
                label.set_markup(&format!(
                    "<b>{}</b>:{}  <span font_family=\"monospace\">{}</span>",
                    glib::markup_escape_text(&doc.name()),
                    line + 1,
                    glib::markup_escape_text(&preview)
                ));
                f.results.append(&label);
                f.hits.borrow_mut().push(Hit { doc_id: doc.id.clone(), start: s, end: e });
            }
        }
        f.results_scroller.set_visible(!f.hits.borrow().is_empty());
    }

    fn jump_to_hit(self: &Rc<Self>, index: usize) {
        let (id, start, end) = {
            let hits = self.find.hits.borrow();
            let Some(h) = hits.get(index) else { return };
            (h.doc_id.clone(), h.start, h.end)
        };
        let Some(doc) = self.docs.borrow().iter().find(|d| d.id == id).cloned() else { return };
        if let Some(page) = self.page_of(&doc) {
            self.tab_view.set_selected_page(&page);
        }
        let buf = &doc.buffer;
        buf.select_range(&buf.iter_at_offset(end), &buf.iter_at_offset(start));
        doc.view.scroll_to_mark(&buf.get_insert(), 0.1, true, 0.0, 0.5);
        doc.view.grab_focus();
    }

    fn step_all_tabs(self: &Rc<Self>, forward: bool) {
        let n = self.find.hits.borrow().len();
        if n == 0 {
            return;
        }
        let cur = self.find.results.selected_row().map_or(if forward { n - 1 } else { 0 }, |r| r.index() as usize);
        let next = if forward { (cur + 1) % n } else { (cur + n - 1) % n };
        if let Some(row) = self.find.results.row_at_index(next as i32) {
            self.find.results.select_row(Some(&row));
        }
        self.jump_to_hit(next);
    }

    async fn replace_all_tabs(self: &Rc<Self>, q: Query, rep: String) {
        let Ok(Some(re)) = compile(&q) else { return };
        let mut per_tab = Vec::new();
        for doc in self.docs_in_order() {
            if doc.state.borrow().read_only {
                continue;
            }
            let n = scan(&doc.text(), &re, usize::MAX).len();
            if n > 0 {
                per_tab.push((doc, n));
            }
        }
        if per_tab.is_empty() {
            self.toast("No matches in any tab");
            return;
        }
        let summary = per_tab.iter().map(|(d, n)| format!("{}: {n}", d.name())).collect::<Vec<_>>().join("\n");
        let total: usize = per_tab.iter().map(|(_, n)| n).sum();
        let a = ask(
            &self.window,
            &format!("Replace {total} matches in {} tabs?", per_tab.len()),
            &summary,
            &[("cancel", "Cancel", Tone::Normal), ("replace", "Replace All", Tone::Destructive)],
            "cancel",
            "cancel",
        )
        .await;
        if a != "replace" {
            return;
        }
        for (doc, _) in &per_tab {
            if q.regex {
                replace_regex_matches(doc, &re, &rep);
            } else {
                configure(doc, &q);
                let _ = replace_all_in(&doc.search, &rep);
            }
        }
        self.toast(&format!("Replaced {total} in {} tabs", per_tab.len()));
        self.run_search();
    }
}
