//! Edit, line, case and view actions.
use super::dialogs::{ask, Tone};
use super::doc::Doc;
use super::window::Notey;
use crate::core::comment::{self, CommentSyntax};
use crate::core::text_ops::{self, IndentStyle, SortOptions};
use adw::prelude::*;
use sourceview5::prelude::*;
use std::rc::Rc;

/// First and last (0-based) lines touched by the selection or cursor. A selection that
/// ends at column 0 of a later line does not include that line.
fn touched_lines(doc: &Doc) -> (i32, i32) {
    let cursor = doc.buffer.iter_at_offset(doc.buffer.cursor_position());
    let (s, e) = doc.buffer.selection_bounds().unwrap_or((cursor, cursor));
    let first = s.line();
    let mut last = e.line();
    if e != s && e.line_offset() == 0 && last > first {
        last -= 1;
    }
    (first, last)
}

fn line_span(doc: &Doc, first: i32, last: i32) -> (i32, i32) {
    let start = doc.buffer.iter_at_line(first).map_or(0, |i| i.offset());
    let mut end = doc.buffer.iter_at_line(last).unwrap_or_else(|| doc.buffer.end_iter());
    if !end.ends_line() {
        end.forward_to_line_end();
    }
    (start, end.offset())
}

fn text_between(doc: &Doc, start: i32, end: i32) -> String {
    doc.buffer.text(&doc.buffer.iter_at_offset(start), &doc.buffer.iter_at_offset(end), true).to_string()
}

/// Replaces `start..end` as one undo step and selects the new text (or keeps the caret
/// where it was when nothing was selected).
fn replace_span(doc: &Doc, start: i32, end: i32, new: &str, select: bool) {
    // Every programmatic edit funnels through here; a read-only tab must not change.
    if !doc.view.is_editable() {
        return;
    }
    let caret = doc.buffer.cursor_position();
    let buf = &doc.buffer;
    buf.begin_user_action();
    buf.delete(&mut buf.iter_at_offset(start), &mut buf.iter_at_offset(end));
    buf.insert(&mut buf.iter_at_offset(start), new);
    buf.end_user_action();
    let new_end = start + new.chars().count() as i32;
    if select {
        buf.select_range(&buf.iter_at_offset(start), &buf.iter_at_offset(new_end));
    } else {
        buf.place_cursor(&buf.iter_at_offset(caret.clamp(start, new_end)));
    }
}

impl Notey {
    fn syntax_for(&self, doc: &Doc) -> CommentSyntax {
        let lang = doc.buffer.language();
        let meta = |k: &str| lang.as_ref().and_then(|l| l.metadata(k)).map(|s| s.to_string());
        let (ls, bs, be) = (meta("line-comment-start"), meta("block-comment-start"), meta("block-comment-end"));
        comment::resolve(
            lang.as_ref().map(|l| l.id().to_string()).as_deref(),
            ls.as_deref(),
            bs.as_deref().zip(be.as_deref()),
        )
    }

    /// Applies a transform to the touched lines; with `whole_if_none` and no selection, to the whole document.
    fn edit_lines(&self, whole_if_none: bool, f: impl Fn(&str) -> Option<String>) {
        let Some(doc) = self.current_doc() else { return };
        if doc.state.borrow().read_only {
            return;
        }
        let had_selection = doc.buffer.selection_bounds().is_some();
        let (first, last) = if whole_if_none && !had_selection {
            (0, (doc.buffer.line_count() - 1).max(0))
        } else {
            touched_lines(&doc)
        };
        let (start, end) = line_span(&doc, first, last);
        let old = text_between(&doc, start, end);
        if let Some(new) = f(&old) {
            if new != old {
                replace_span(&doc, start, end, &new, had_selection || whole_if_none);
            }
        }
    }

    fn edit_selection_or_word(&self, f: impl Fn(&str) -> String) {
        let Some(doc) = self.current_doc() else { return };
        let buf = &doc.buffer;
        let (s, e) = match buf.selection_bounds() {
            Some(b) => b,
            None => {
                let mut s = buf.iter_at_offset(buf.cursor_position());
                let mut e = s;
                if !s.starts_word() {
                    s.backward_word_start();
                }
                if !e.ends_word() {
                    e.forward_word_end();
                }
                (s, e)
            }
        };
        let (so, eo) = (s.offset(), e.offset());
        let old = text_between(&doc, so, eo);
        let new = f(&old);
        if new != old {
            replace_span(&doc, so, eo, &new, true);
        }
    }

    /// Comment (`Some(true)`) or uncomment (`Some(false)`) the touched lines; never the opposite.
    fn set_comment(&self, comment_them: bool) {
        let Some(doc) = self.current_doc() else { return };
        let syntax = self.syntax_for(&doc);
        let (first, last) = touched_lines(&doc);
        let (start, end) = line_span(&doc, first, last);
        match comment::line_comment_state(&text_between(&doc, start, end), &syntax) {
            None if syntax.is_empty() => self.toast("This language has no comment syntax"),
            None => {}
            Some(all) if all != comment_them => self.toggle_comment(false),
            Some(_) => {}
        }
    }

    fn toggle_comment(&self, block: bool) {
        let Some(doc) = self.current_doc() else { return };
        let syntax = self.syntax_for(&doc);
        let no_syntax = || self.toast("This language has no comment syntax");
        if block {
            let (start, end, select) = match doc.buffer.selection_bounds() {
                Some((s, e)) => (s.offset(), e.offset(), true),
                None => {
                    let (a, b) = line_span(&doc, doc.cursor().line as i32, doc.cursor().line as i32);
                    (a, b, false)
                }
            };
            let old = text_between(&doc, start, end);
            match comment::toggle_block_comment(&old, &syntax) {
                Some(new) => replace_span(&doc, start, end, &new, select),
                None => no_syntax(),
            }
        } else {
            if syntax.is_empty() {
                no_syntax();
                return;
            }
            self.edit_lines(false, |t| comment::toggle_line_comment(t, &syntax));
        }
    }

    fn indent_unit(&self, doc: &Doc) -> IndentStyle {
        let n = doc.view.indent_width().max(1) as usize;
        if doc.view.is_insert_spaces_instead_of_tabs() {
            IndentStyle::Spaces(n)
        } else {
            IndentStyle::Tabs
        }
    }

    fn move_line(&self, up: bool) {
        let Some(doc) = self.current_doc() else { return };
        let (first, last) = touched_lines(&doc);
        let total = doc.buffer.line_count();
        if (up && first == 0) || (!up && last + 1 >= total) {
            return;
        }
        let (rf, rl) = if up { (first - 1, last) } else { (first, last + 1) };
        let (start, end) = line_span(&doc, rf, rl);
        let mut lines: Vec<String> = text_between(&doc, start, end).split('\n').map(String::from).collect();
        let n = (last - first) as usize;
        let moved = if up { text_ops::move_lines(&mut lines, 1, 1 + n, true) } else { text_ops::move_lines(&mut lines, 0, n, false) };
        if moved {
            let had = doc.buffer.selection_bounds().is_some();
            replace_span(&doc, start, end, &lines.join("\n"), true);
            if !had {
                let line = if up { first - 1 } else { first + 1 };
                if let Some(it) = doc.buffer.iter_at_line(line) {
                    doc.buffer.place_cursor(&it);
                }
            }
        }
    }

    fn delete_lines(&self) {
        let Some(doc) = self.current_doc() else { return };
        let (first, last) = touched_lines(&doc);
        let (mut start, mut end) = line_span(&doc, first, last);
        if end < doc.buffer.end_iter().offset() {
            end += 1; // the line break
        } else if start > 0 {
            start -= 1; // last line: take the preceding break instead
        }
        replace_span(&doc, start, end, "", false);
    }

    fn join_lines(&self) {
        let Some(doc) = self.current_doc() else { return };
        let (first, mut last) = touched_lines(&doc);
        if first == last {
            if last + 1 >= doc.buffer.line_count() {
                return;
            }
            last += 1;
        }
        let (start, end) = line_span(&doc, first, last);
        let old = text_between(&doc, start, end);
        replace_span(&doc, start, end, &text_ops::join_lines(&old), false);
    }

    fn cut_copy_line_hook(self: &Rc<Self>, doc: &Rc<Doc>) {
        let weak = Rc::downgrade(self);
        let key = gtk::EventControllerKey::new();
        key.set_propagation_phase(gtk::PropagationPhase::Capture);
        let wd = Rc::downgrade(doc);
        key.connect_key_pressed(move |_, keyval, _, state| {
            let mods = state & gtk::accelerator_get_default_mod_mask();
            if mods != gtk::gdk::ModifierType::CONTROL_MASK || !matches!(keyval, gtk::gdk::Key::x | gtk::gdk::Key::c) {
                return glib::Propagation::Proceed;
            }
            if let (Some(t), Some(d)) = (weak.upgrade(), wd.upgrade()) {
                if t.settings.borrow().cut_line_when_empty && d.buffer.selection_bounds().is_none() && d.buffer.char_count() > 0 {
                    let line = d.cursor().line as i32;
                    let (s, e) = line_span(&d, line, line);
                    let end = (e + 1).min(d.buffer.end_iter().offset());
                    d.buffer.select_range(&d.buffer.iter_at_offset(s), &d.buffer.iter_at_offset(end));
                }
            }
            glib::Propagation::Proceed
        });
        doc.view.add_controller(key);
    }

    pub fn on_doc_added_editing(self: &Rc<Self>, doc: &Rc<Doc>) {
        self.cut_copy_line_hook(doc);
    }

    fn add_simple(self: &Rc<Self>, name: &str, f: impl Fn(&Rc<Notey>) + 'static) {
        let a = gio::SimpleAction::new(name, None);
        let weak = Rc::downgrade(self);
        a.connect_activate(move |_, _| {
            if let Some(t) = weak.upgrade() {
                f(&t);
            }
        });
        self.window.add_action(&a);
    }

    fn add_toggle(self: &Rc<Self>, name: &str, initial: bool, set: impl Fn(&mut crate::settings::Settings, bool) + 'static) {
        let a = gio::SimpleAction::new_stateful(name, None, &initial.to_variant());
        let weak = Rc::downgrade(self);
        a.connect_activate(move |a, _| {
            let Some(t) = weak.upgrade() else { return };
            let on = !a.state().and_then(|s| s.get::<bool>()).unwrap_or(false);
            a.set_state(&on.to_variant());
            set(&mut t.settings.borrow_mut(), on);
            t.apply_settings_everywhere();
        });
        self.window.add_action(&a);
    }

    fn zoom(&self, delta: i32) {
        {
            let mut s = self.settings.borrow_mut();
            s.zoom_percent = if delta == 0 { 100 } else { (s.zoom_percent as i32 + delta).clamp(50, 400) as u32 };
        }
        self.apply_settings_everywhere();
    }

    pub fn install_editing_actions(self: &Rc<Self>) {
        use text_ops as t;
        let s = self.settings.borrow().clone();
        self.add_toggle("word-wrap", s.word_wrap, |s, v| s.word_wrap = v);
        self.add_toggle("show-whitespace", s.show_whitespace, |s, v| s.show_whitespace = v);
        self.add_toggle("show-minimap", s.show_minimap, |s, v| s.show_minimap = v);
        self.add_toggle("line-numbers", s.line_numbers, |s, v| s.line_numbers = v);
        self.add_simple("zoom-in", |w| w.zoom(10));
        self.add_simple("zoom-out", |w| w.zoom(-10));
        self.add_simple("zoom-reset", |w| w.zoom(0));

        for (name, op) in [("undo", 0), ("redo", 1), ("cut", 2), ("copy", 3), ("paste", 4), ("select-all", 5)] {
            self.add_simple(name, move |w| {
                let Some(d) = w.current_doc() else { return };
                match op {
                    0 => d.buffer.undo(),
                    1 => d.buffer.redo(),
                    2 => d.view.emit_by_name::<()>("cut-clipboard", &[]),
                    3 => d.view.emit_by_name::<()>("copy-clipboard", &[]),
                    4 => d.view.emit_by_name::<()>("paste-clipboard", &[]),
                    _ => d.view.emit_by_name::<()>("select-all", &[&true]),
                }
                d.view.grab_focus();
            });
        }

        self.add_toggle("show-toolbar", s.show_toolbar, |s, v| s.show_toolbar = v);
        self.add_simple("comment-lines", |w| w.set_comment(true));
        self.add_simple("uncomment-lines", |w| w.set_comment(false));
        self.add_simple("toggle-comment", |w| w.toggle_comment(false));
        self.add_simple("toggle-block-comment", |w| w.toggle_comment(true));
        self.add_simple("duplicate-line", |w| w.edit_lines(false, |x| Some(t::duplicate_lines(x))));
        self.add_simple("delete-line", |w| w.delete_lines());
        self.add_simple("move-line-up", |w| w.move_line(true));
        self.add_simple("move-line-down", |w| w.move_line(false));
        self.add_simple("join-lines", |w| w.join_lines());
        self.add_simple("sort-asc", |w| w.edit_lines(true, |x| Some(t::sort_lines(x, SortOptions::default()))));
        self.add_simple("sort-desc", |w| w.edit_lines(true, |x| Some(t::sort_lines(x, SortOptions { descending: true, ..Default::default() }))));
        self.add_simple("sort-ci", |w| w.edit_lines(true, |x| Some(t::sort_lines(x, SortOptions { case_insensitive: true, ..Default::default() }))));
        self.add_simple("sort-numeric", |w| w.edit_lines(true, |x| Some(t::sort_lines(x, SortOptions { numeric: true, case_insensitive: true, ..Default::default() }))));
        self.add_simple("dedupe-lines", |w| w.edit_lines(true, |x| Some(t::dedupe_lines(x))));
        self.add_simple("remove-blank-lines", |w| w.edit_lines(true, |x| Some(t::remove_blank_lines(x))));
        self.add_simple("reverse-lines", |w| w.edit_lines(true, |x| Some(t::reverse_lines(x))));
        self.add_simple("trim-trailing", |w| w.edit_lines(true, |x| Some(t::trim_trailing(x))));
        self.add_simple("trim-leading", |w| w.edit_lines(true, |x| Some(t::trim_leading(x))));
        self.add_simple("indent", |w| {
            if let Some(d) = w.current_doc() {
                let unit = w.indent_unit(&d);
                w.edit_lines(false, |x| Some(t::indent(x, unit)));
            }
        });
        self.add_simple("unindent", |w| {
            if let Some(d) = w.current_doc() {
                let width = d.view.indent_width().max(1) as usize;
                w.edit_lines(false, |x| Some(t::unindent(x, width)));
            }
        });
        self.add_simple("upper-case", |w| w.edit_selection_or_word(t::to_upper));
        self.add_simple("lower-case", |w| w.edit_selection_or_word(t::to_lower));
        self.add_simple("title-case", |w| w.edit_selection_or_word(t::to_title));
        self.add_simple("swap-case", |w| w.edit_selection_or_word(t::swap_case));
        self.add_simple("insert-datetime", |w| {
            if let Some(d) = w.current_doc().filter(|d| d.view.is_editable()) {
                if let Ok(now) = glib::DateTime::now_local() {
                    if let Ok(text) = now.format("%Y-%m-%d %H:%M:%S") {
                        d.buffer.insert_at_cursor(&text);
                    }
                }
            }
        });
        self.add_simple("goto-line", |w| w.goto_line_dialog());
        self.add_simple("statistics", |w| w.statistics_dialog());
    }

    fn goto_line_dialog(self: &Rc<Self>) {
        let Some(doc) = self.current_doc() else { return };
        let this = self.clone();
        glib::spawn_future_local(async move {
            let total = doc.buffer.line_count();
            let dialog = adw::MessageDialog::new(Some(&this.window), Some("Go to Line"), Some(&format!("Line number, or LINE:COLUMN (1–{total})")));
            let entry = gtk::Entry::builder().activates_default(true).input_purpose(gtk::InputPurpose::Digits).build();
            let cur = doc.cursor();
            entry.set_text(&format!("{}", cur.line + 1));
            entry.select_region(0, -1);
            dialog.set_extra_child(Some(&entry));
            dialog.add_response("cancel", "Cancel");
            dialog.add_response("go", "Go");
            dialog.set_response_appearance("go", adw::ResponseAppearance::Suggested);
            dialog.set_default_response(Some("go"));
            dialog.set_close_response("cancel");
            entry.grab_focus();
            if dialog.choose_future().await != "go" {
                return;
            }
            let text = entry.text();
            let mut parts = text.trim().splitn(2, ':');
            let line = parts.next().and_then(|p| p.trim().parse::<u32>().ok());
            let col = parts.next().and_then(|p| p.trim().parse::<u32>().ok());
            if let Some(line) = line {
                this.goto_line(&doc, line.max(1), col);
            }
        });
    }

    fn statistics_dialog(self: &Rc<Self>) {
        let Some(doc) = self.current_doc() else { return };
        let (text, scope) = match doc.buffer.selection_bounds() {
            Some((s, e)) => (doc.buffer.text(&s, &e, true).to_string(), "Selection"),
            None => (doc.text(), "Document"),
        };
        let st = doc.state.borrow();
        let bytes = crate::core::encoding::encode(&text, st.encoding, st.bom, st.line_ending).bytes.len();
        drop(st);
        let body = format!(
            "Lines: {}\nWords: {}\nCharacters: {}\nCharacters without spaces: {}\nSize: {} bytes ({})",
            if text.is_empty() { 0 } else { text.split('\n').count() },
            text.split_whitespace().count(),
            text.chars().count(),
            text.chars().filter(|c| !c.is_whitespace()).count(),
            bytes,
            doc.state.borrow().encoding.label(),
        );
        let this = self.clone();
        glib::spawn_future_local(async move {
            ask(&this.window, &format!("{scope} Statistics"), &body, &[("ok", "Close", Tone::Suggested)], "ok", "ok").await;
        });
    }
}
