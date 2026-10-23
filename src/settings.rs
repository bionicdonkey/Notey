//! User preferences, stored as JSON in `~/.config/notey/settings.json`.
//! Unknown keys are preserved so a newer or older build does not drop them.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const MIN_INDENT_WIDTH: u32 = 1;
pub const MAX_INDENT_WIDTH: u32 = 16;
pub const DEFAULT_INDENT_WIDTH: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeChoice {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    // Editor
    pub font: String,
    pub font_size: u32,
    /// Width of one indent level, in columns. Also the tab-stop display width.
    pub indent_width: u32,
    pub insert_spaces: bool,
    pub auto_indent: bool,
    pub word_wrap: bool,
    pub line_numbers: bool,
    pub highlight_current_line: bool,
    pub show_whitespace: bool,
    pub right_margin: u32,
    pub auto_close_brackets: bool,
    pub cut_line_when_empty: bool,
    pub ensure_final_newline: bool,
    // Appearance
    pub theme: ThemeChoice,
    pub style_scheme: Option<String>,
    pub show_minimap: bool,
    pub show_toolbar: bool,
    /// Ids from `ui::menus::QUICK_ACTIONS`, in display order.
    pub quick_actions: Vec<String>,
    pub zoom_percent: u32,
    // Files
    pub default_encoding: String,
    pub default_utf8_bom: bool,
    pub default_line_ending: crate::core::line_ending::LineEnding,
    pub recent_size: usize,
    // Session
    pub remember_tabs: bool,
    pub remember_search_history: bool,
    // Window and search state
    pub window_width: i32,
    pub window_height: i32,
    pub window_maximized: bool,
    pub search_case_sensitive: bool,
    pub search_whole_word: bool,
    pub search_regex: bool,
    pub search_history: Vec<String>,
    pub replace_history: Vec<String>,
    pub recent_files: Vec<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            font: "Monospace".into(),
            font_size: 11,
            indent_width: DEFAULT_INDENT_WIDTH,
            insert_spaces: true,
            auto_indent: true,
            word_wrap: false,
            line_numbers: true,
            highlight_current_line: true,
            show_whitespace: false,
            right_margin: 0,
            auto_close_brackets: false,
            cut_line_when_empty: true,
            ensure_final_newline: false,
            theme: ThemeChoice::System,
            style_scheme: None,
            show_minimap: false,
            show_toolbar: true,
            quick_actions: crate::ui::menus::default_quick_actions(),
            zoom_percent: 100,
            default_encoding: "UTF-8".into(),
            default_utf8_bom: false,
            default_line_ending: Default::default(),
            recent_size: 15,
            remember_tabs: true,
            remember_search_history: true,
            window_width: 960,
            window_height: 680,
            window_maximized: false,
            search_case_sensitive: false,
            search_whole_word: false,
            search_regex: false,
            search_history: Vec::new(),
            replace_history: Vec::new(),
            recent_files: Vec::new(),
            extra: Default::default(),
        }
    }
}

impl Settings {
    /// Pulls out-of-range values back to something usable.
    pub fn clamped(mut self) -> Self {
        self.indent_width = self.indent_width.clamp(MIN_INDENT_WIDTH, MAX_INDENT_WIDTH);
        self.zoom_percent = self.zoom_percent.clamp(50, 400);
        self.font_size = self.font_size.clamp(6, 72);
        self.recent_size = self.recent_size.clamp(1, 50);
        self
    }

    /// A damaged file yields defaults instead of an error: preferences must never stop the app.
    pub fn from_json(json: &str) -> Self {
        serde_json::from_str::<Settings>(json).unwrap_or_default().clamped()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("settings serialise")
    }

    pub fn path() -> PathBuf {
        if let Some(d) = std::env::var_os("NOTEY_CONFIG_DIR") {
            return PathBuf::from(d).join("settings.json");
        }
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or_else(|| PathBuf::from("."))
            .join("notey/settings.json")
    }

    pub fn load() -> Self {
        std::fs::read_to_string(Self::path()).map(|s| Self::from_json(&s)).unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        crate::application::fs_util::atomic_write(&path, self.to_json().as_bytes(), None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indent_width_defaults_to_four() {
        assert_eq!(Settings::default().indent_width, 4);
        assert!(Settings::default().insert_spaces);
    }

    #[test]
    fn indent_width_is_configurable_and_clamped() {
        assert_eq!(Settings::from_json(r#"{"indent_width":2}"#).indent_width, 2);
        assert_eq!(Settings::from_json(r#"{"indent_width":0}"#).indent_width, 1);
        assert_eq!(Settings::from_json(r#"{"indent_width":99}"#).indent_width, 16);
    }

    #[test]
    fn partial_and_damaged_files_fall_back_to_defaults() {
        let s = Settings::from_json(r#"{"word_wrap":true}"#);
        assert!(s.word_wrap);
        assert_eq!(s.indent_width, 4);
        assert_eq!(Settings::from_json("{ nope").indent_width, 4);
        assert_eq!(Settings::from_json(r#"{"indent_width":"four"}"#).indent_width, 4);
    }

    #[test]
    fn unknown_keys_survive_a_round_trip() {
        let s = Settings::from_json(r#"{"from_the_future":{"a":1}}"#);
        let again = Settings::from_json(&s.to_json());
        assert_eq!(again.extra["from_the_future"]["a"], 1);
    }

    #[test]
    fn save_and_load() {
        let d = tempfile::tempdir().unwrap();
        std::env::set_var("NOTEY_CONFIG_DIR", d.path());
        let s = Settings { indent_width: 8, ..Default::default() };
        s.save().unwrap();
        assert_eq!(Settings::load().indent_width, 8);
        std::env::remove_var("NOTEY_CONFIG_DIR");
    }
}
