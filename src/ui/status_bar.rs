use gtk::prelude::*;

pub struct StatusBar {
    pub root: gtk::Box,
    pub position: gtk::Label,
    pub note: gtk::Label,
    pub language: gtk::MenuButton,
    pub indent: gtk::MenuButton,
    pub encoding: gtk::MenuButton,
    pub line_ending: gtk::MenuButton,
    pub zoom: gtk::Button,
}

fn flat_menu_button(tooltip: &str) -> gtk::MenuButton {
    let b = gtk::MenuButton::new();
    b.set_has_frame(false);
    b.set_direction(gtk::ArrowType::Up);
    b.set_tooltip_text(Some(tooltip));
    b
}

impl StatusBar {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        root.add_css_class("notey-statusbar");
        let position = gtk::Label::new(Some("Ln 1, Col 1"));
        position.set_xalign(0.0);
        position.set_margin_start(6);
        position.set_hexpand(true);
        position.set_selectable(false);
        let note = gtk::Label::new(None);
        note.add_css_class("dim-label");
        let language = flat_menu_button("Language");
        let indent = flat_menu_button("Indentation");
        let encoding = flat_menu_button("Encoding");
        let line_ending = flat_menu_button("Line endings");
        let zoom = gtk::Button::with_label("100%");
        zoom.set_has_frame(false);
        zoom.set_tooltip_text(Some("Reset zoom"));
        zoom.set_action_name(Some("win.zoom-reset"));
        for w in [
            position.upcast_ref::<gtk::Widget>(),
            note.upcast_ref(),
            language.upcast_ref(),
            indent.upcast_ref(),
            encoding.upcast_ref(),
            line_ending.upcast_ref(),
            zoom.upcast_ref(),
        ] {
            root.append(w);
        }
        Self { root, position, note, language, indent, encoding, line_ending, zoom }
    }
}

impl Default for StatusBar {
    fn default() -> Self {
        Self::new()
    }
}
