//! The traditional menu bar and the quick-actions toolbar.
use super::window::Notey;
use gtk::prelude::*;

fn section(items: &[(&str, &str)]) -> gio::Menu {
    let m = gio::Menu::new();
    for (label, action) in items {
        m.append(Some(label), Some(&format!("win.{action}")));
    }
    m
}

fn menu(sections: Vec<gio::Menu>) -> gio::Menu {
    let m = gio::Menu::new();
    for s in sections {
        m.append_section(None, &s);
    }
    m
}

/// File, Edit, Search and View as submenus of the main menu, accelerators shown beside each item.
pub fn main_menu_model(recent: &gio::Menu) -> gio::Menu {
    let file_open = gio::Menu::new();
    file_open.append(Some("New Tab"), Some("win.new-tab"));
    file_open.append(Some("Open…"), Some("win.open"));
    file_open.append_submenu(Some("Recent Files"), recent);
    file_open.append(Some("Reopen Closed Tab"), Some("win.reopen-closed"));
    let file = menu(vec![
        file_open,
        section(&[("Save", "save"), ("Save As…", "save-as"), ("Save All", "save-all")]),
        section(&[("Reload from Disk", "reload")]),
        section(&[("Close Tab", "close-tab"), ("Quit", "quit")]),
    ]);

    let comments = section(&[
        ("Comment Lines", "comment-lines"),
        ("Uncomment Lines", "uncomment-lines"),
        ("Toggle Comment", "toggle-comment"),
        ("Toggle Block Comment", "toggle-block-comment"),
    ]);
    let lines = section(&[
        ("Duplicate Line", "duplicate-line"),
        ("Delete Line", "delete-line"),
        ("Move Line Up", "move-line-up"),
        ("Move Line Down", "move-line-down"),
        ("Join Lines", "join-lines"),
        ("Indent", "indent"),
        ("Unindent", "unindent"),
    ]);
    let sort = section(&[
        ("Sort Ascending", "sort-asc"),
        ("Sort Descending", "sort-desc"),
        ("Sort Ignoring Case", "sort-ci"),
        ("Sort Numerically", "sort-numeric"),
        ("Remove Duplicate Lines", "dedupe-lines"),
        ("Remove Blank Lines", "remove-blank-lines"),
        ("Reverse Lines", "reverse-lines"),
        ("Trim Trailing Whitespace", "trim-trailing"),
        ("Trim Leading Whitespace", "trim-leading"),
    ]);
    let case = section(&[("UPPERCASE", "upper-case"), ("lowercase", "lower-case"), ("Title Case", "title-case"), ("Swap Case", "swap-case")]);
    let submenus = gio::Menu::new();
    submenus.append_submenu(Some("Lines"), &lines);
    submenus.append_submenu(Some("Sort and Clean Up"), &sort);
    submenus.append_submenu(Some("Change Case"), &case);
    let edit = menu(vec![
        section(&[("Undo", "undo"), ("Redo", "redo")]),
        section(&[("Cut", "cut"), ("Copy", "copy"), ("Paste", "paste"), ("Select All", "select-all")]),
        comments,
        submenus,
        section(&[("Insert Date and Time", "insert-datetime"), ("Go to Line…", "goto-line"), ("Document Statistics", "statistics")]),
    ]);

    let search = menu(vec![section(&[
        ("Find…", "find"),
        ("Find and Replace…", "find-replace"),
        ("Find Next", "find-next"),
        ("Find Previous", "find-prev"),
    ])]);

    let view = menu(vec![
        section(&[("Word Wrap", "word-wrap"), ("Show Whitespace", "show-whitespace"), ("Line Numbers", "line-numbers"), ("Minimap", "show-minimap")]),
        section(&[("Zoom In", "zoom-in"), ("Zoom Out", "zoom-out"), ("Reset Zoom", "zoom-reset")]),
        section(&[("Quick Actions Bar", "show-toolbar"), ("Customize Quick Actions…", "customize-quick")]),
    ]);

    let app = section(&[("Preferences", "preferences"), ("Keyboard Shortcuts", "shortcuts"), ("About NoteyMcNoteface", "about")]);

    let top = gio::Menu::new();
    let groups = gio::Menu::new();
    groups.append_submenu(Some("File"), &file);
    groups.append_submenu(Some("Edit"), &edit);
    groups.append_submenu(Some("Search"), &search);
    groups.append_submenu(Some("View"), &view);
    top.append_section(None, &groups);
    top.append_section(None, &app);
    top
}

pub struct QuickAction {
    pub id: &'static str,
    pub tip: &'static str,
    pub action: &'static str,
    /// A symbolic icon name, or a markup glyph when `icon` is `None`.
    pub icon: Option<&'static str>,
    pub glyph: &'static str,
}

const fn qa(id: &'static str, tip: &'static str, action: &'static str, icon: &'static str) -> QuickAction {
    QuickAction { id, tip, action, icon: Some(icon), glyph: "" }
}

/// Everything that can sit on the quick bar, in the order shown in Preferences.
pub const QUICK_ACTIONS: &[QuickAction] = &[
    qa("new", "New tab", "new-tab", "tab-new-symbolic"),
    qa("open", "Open…", "open", "document-open-symbolic"),
    qa("save", "Save", "save", "document-save-symbolic"),
    qa("save-all", "Save all", "save-all", "document-save-as-symbolic"),
    qa("undo", "Undo", "undo", "edit-undo-symbolic"),
    qa("redo", "Redo", "redo", "edit-redo-symbolic"),
    qa("cut", "Cut", "cut", "edit-cut-symbolic"),
    qa("copy", "Copy", "copy", "edit-copy-symbolic"),
    qa("paste", "Paste", "paste", "edit-paste-symbolic"),
    QuickAction { id: "comment", tip: "Comment lines", action: "comment-lines", icon: None, glyph: "//" },
    QuickAction { id: "uncomment", tip: "Uncomment lines", action: "uncomment-lines", icon: None, glyph: "×//" },
    qa("indent", "Indent", "indent", "format-indent-more-symbolic"),
    qa("unindent", "Unindent", "unindent", "format-indent-less-symbolic"),
    qa("find", "Find (Ctrl+F)", "find", "edit-find-symbolic"),
    qa("replace", "Find and replace (Ctrl+H)", "find-replace", "edit-find-replace-symbolic"),
    qa("goto", "Go to line (Ctrl+G)", "goto-line", "go-jump-symbolic"),
    qa("duplicate", "Duplicate line", "duplicate-line", "edit-copy-symbolic"),
    qa("delete-line", "Delete line", "delete-line", "edit-delete-symbolic"),
    qa("sort", "Sort lines", "sort-asc", "view-sort-ascending-symbolic"),
    qa("upper", "UPPERCASE", "upper-case", "format-text-uppercase-symbolic"),
    qa("lower", "lowercase", "lower-case", "format-text-lowercase-symbolic"),
    qa("wrap", "Word wrap (Alt+Z)", "word-wrap", "format-justify-fill-symbolic"),
    qa("zoom-in", "Zoom in", "zoom-in", "zoom-in-symbolic"),
    qa("zoom-out", "Zoom out", "zoom-out", "zoom-out-symbolic"),
    qa("reload", "Reload from disk", "reload", "view-refresh-symbolic"),
];

/// A divider between groups on the quick bar. It may appear any number of times in the list.
pub const SEPARATOR: &str = "|";

pub fn default_quick_actions() -> Vec<String> {
    [
        "undo", "redo", "|", "cut", "copy", "paste", "|", "comment", "uncomment", "indent", "unindent", "|", "find",
        "replace",
    ]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

pub fn quick_action(id: &str) -> Option<&'static QuickAction> {
    QUICK_ACTIONS.iter().find(|a| a.id == id)
}

fn quick_button(a: &QuickAction) -> gtk::Button {
    let b = match a.icon {
        Some(name) => gtk::Button::from_icon_name(name),
        None => {
            let l = gtk::Label::new(None);
            l.set_markup(&format!("<b>{}</b>", a.glyph));
            let b = gtk::Button::new();
            b.set_child(Some(&l));
            b
        }
    };
    b.set_tooltip_text(Some(a.tip));
    b.set_action_name(Some(&format!("win.{}", a.action)));
    b.add_css_class("flat");
    b.update_property(&[gtk::accessible::Property::Label(a.tip)]);
    b
}

/// Index in `list` of the n-th entry that is a separator or a known action (what Preferences shows).
pub fn nth_known(list: &[String], n: usize) -> Option<usize> {
    list.iter().enumerate().filter(|(_, x)| *x == SEPARATOR || quick_action(x).is_some()).nth(n).map(|(p, _)| p)
}

/// Drag-and-drop move: the entry shown at row `from` takes the place of the one shown at row `to`.
pub fn move_entry(list: &mut Vec<String>, from: usize, to: usize) {
    let Some(p) = nth_known(list, from) else { return };
    if from == to {
        return;
    }
    let item = list.remove(p);
    let at = nth_known(list, to).unwrap_or(list.len());
    list.insert(at, item);
}

fn quick_row(ids: &[String]) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    row.add_css_class("notey-quickbar");
    // Separators only divide: none at the ends, never two in a row.
    let mut pending_separator = false;
    let mut any = false;
    for id in ids {
        if id == SEPARATOR {
            pending_separator = any;
            continue;
        }
        let Some(a) = quick_action(id) else { continue };
        if pending_separator {
            let sep = gtk::Separator::new(gtk::Orientation::Vertical);
            sep.set_margin_top(4);
            sep.set_margin_bottom(4);
            sep.set_margin_start(4);
            sep.set_margin_end(4);
            row.append(&sep);
            pending_separator = false;
        }
        row.append(&quick_button(a));
        any = true;
    }
    row
}

/// A slim, icons-only row built from the ids in settings, scrollable on a narrow window.
pub fn quick_actions(ids: &[String]) -> gtk::ScrolledWindow {
    let row = quick_row(ids);
    let scroller = gtk::ScrolledWindow::builder()
        .child(&row)
        .hscrollbar_policy(gtk::PolicyType::External)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .build();
    scroller.add_css_class("notey-quickbar-scroller");
    scroller
}

impl Notey {
    /// Shows or hides the quick bar and rebuilds it when the chosen actions changed.
    pub fn apply_chrome_visibility(&self) {
        let s = self.settings.borrow();
        self.quickbar.set_visible(s.show_toolbar);
        let key = s.quick_actions.join(",");
        if *self.quick_key.borrow() != key {
            self.quickbar.set_child(Some(&quick_row(&s.quick_actions)));
            *self.quick_key.borrow_mut() = key;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn dragging_down_lands_after_the_target() {
        let mut l = v(&["undo", "redo", "|", "cut"]);
        move_entry(&mut l, 0, 2);
        assert_eq!(l, v(&["redo", "|", "undo", "cut"]));
    }

    #[test]
    fn dragging_up_lands_before_the_target() {
        let mut l = v(&["undo", "redo", "|", "cut"]);
        move_entry(&mut l, 3, 1);
        assert_eq!(l, v(&["undo", "cut", "redo", "|"]));
    }

    #[test]
    fn to_the_very_end_and_noops() {
        let mut l = v(&["undo", "redo", "cut"]);
        move_entry(&mut l, 0, 2);
        assert_eq!(l, v(&["redo", "cut", "undo"]));
        move_entry(&mut l, 1, 1);
        move_entry(&mut l, 9, 0);
        assert_eq!(l, v(&["redo", "cut", "undo"]));
    }

    #[test]
    fn unknown_ids_are_skipped_when_counting_rows() {
        let mut l = v(&["gone", "undo", "redo"]);
        move_entry(&mut l, 0, 1);
        assert_eq!(l, v(&["gone", "redo", "undo"]));
    }
}
