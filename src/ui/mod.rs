pub mod dialogs;
pub mod doc;
pub mod editing;
pub mod files;
pub mod find_bar;
pub mod menus;
pub mod pickers;
pub mod preferences;
pub mod status_bar;
pub mod tabs_menu;
pub mod window;

pub fn install_css() {
    if let Some(display) = gtk::gdk::Display::default() {
        let provider = gtk::CssProvider::new();
        provider.load_from_string(include_str!("../../resources/style.css"));
        gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    }
}

/// Every shortcut: (group, title, action, accelerators). Feeds the accelerators and the shortcuts window.
pub const SHORTCUTS: &[(&str, &str, &str, &[&str])] = &[
    ("Tabs and files", "New tab", "win.new-tab", &["<Ctrl>t", "<Ctrl>n"]),
    ("Tabs and files", "Open…", "win.open", &["<Ctrl>o"]),
    ("Tabs and files", "Save", "win.save", &["<Ctrl>s"]),
    ("Tabs and files", "Save As…", "win.save-as", &["<Ctrl><Shift>s"]),
    ("Tabs and files", "Save all", "win.save-all", &["<Ctrl><Alt>s"]),
    ("Tabs and files", "Close tab", "win.close-tab", &["<Ctrl>w"]),
    ("Tabs and files", "Reopen closed tab", "win.reopen-closed", &["<Ctrl><Shift>t"]),
    ("Tabs and files", "Reload from disk", "win.reload", &["<Ctrl>r"]),
    ("Tabs and files", "Next tab", "win.next-tab", &["<Ctrl>Tab", "<Ctrl>Page_Down"]),
    ("Tabs and files", "Previous tab", "win.prev-tab", &["<Ctrl><Shift>Tab", "<Ctrl>Page_Up"]),
    ("Tabs and files", "Quit", "win.quit", &["<Ctrl>q"]),
    ("Editing", "Toggle comment", "win.toggle-comment", &["<Ctrl>slash"]),
    ("Editing", "Toggle block comment", "win.toggle-block-comment", &["<Ctrl><Shift>a"]),
    ("Editing", "Duplicate line", "win.duplicate-line", &["<Ctrl><Shift>d"]),
    ("Editing", "Delete line", "win.delete-line", &["<Ctrl><Shift>k"]),
    ("Editing", "Move line up", "win.move-line-up", &["<Alt>Up"]),
    ("Editing", "Move line down", "win.move-line-down", &["<Alt>Down"]),
    ("Editing", "Join lines", "win.join-lines", &["<Ctrl>j"]),
    ("Editing", "Go to line…", "win.goto-line", &["<Ctrl>g"]),
    ("Editing", "Insert date and time", "win.insert-datetime", &["F5"]),
    ("Search", "Find", "win.find", &["<Ctrl>f"]),
    ("Search", "Find and replace", "win.find-replace", &["<Ctrl>h"]),
    ("Search", "Next match", "win.find-next", &["F3"]),
    ("Search", "Previous match", "win.find-prev", &["<Shift>F3"]),
    ("View", "Word wrap", "win.word-wrap", &["<Alt>z"]),
    ("View", "Zoom in", "win.zoom-in", &["<Ctrl>equal", "<Ctrl>plus", "<Ctrl>KP_Add"]),
    ("View", "Zoom out", "win.zoom-out", &["<Ctrl>minus", "<Ctrl>KP_Subtract"]),
    ("View", "Reset zoom", "win.zoom-reset", &["<Ctrl>0"]),
    ("Help", "Keyboard shortcuts", "win.shortcuts", &["<Ctrl>question"]),
];

pub fn install_accels() {
    let app = gio::Application::default().and_downcast::<gtk::Application>();
    let Some(app) = app else { return };
    use gtk::prelude::*;
    for (_, _, action, keys) in SHORTCUTS {
        app.set_accels_for_action(action, keys);
    }
    for n in 1..=9 {
        app.set_accels_for_action(&format!("win.goto-tab({n})"), &[&format!("<Alt>{n}")]);
    }
}

pub const APP_ID: &str = "com.zorin.Notey";

/// Makes the app icon resolvable and the default for every window. An installed package finds
/// it in the system icon theme; a build run from the source tree has not installed anything, so
/// the checkout's `resources/icons` (debug builds) or `NOTEY_ICON_DIR` is added to the search path.
pub fn install_icons() {
    if let Some(display) = gtk::gdk::Display::default() {
        let theme = gtk::IconTheme::for_display(&display);
        if let Some(dir) = std::env::var_os("NOTEY_ICON_DIR") {
            theme.add_search_path(dir);
        }
        #[cfg(debug_assertions)]
        theme.add_search_path(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/icons"));
    }
    gtk::Window::set_default_icon_name(APP_ID);
}
