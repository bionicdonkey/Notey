//! The persisted session: which tabs were open and what they held. This module
//! is the data model, parsing and reconciliation; writing it atomically lives in
//! `application::session_store`.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SESSION_VERSION: u32 = 1;
pub const MAX_CLOSED_TABS: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Cursor {
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    pub start: Cursor,
    pub end: Cursor,
}

/// What the file on disk looked like when the buffer was last in sync with it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskState {
    pub mtime_ns: i64,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TabRecord {
    pub id: String,
    /// `None` for an untitled tab.
    pub path: Option<String>,
    pub title: String,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub dirty: bool,
    /// File name inside `buffers/` holding the unsaved text.
    #[serde(default)]
    pub buffer_file: Option<String>,
    #[serde(default)]
    pub base: Option<DiskState>,
    pub encoding: String,
    #[serde(default)]
    pub bom: bool,
    pub line_ending: crate::core::line_ending::LineEnding,
    /// `None` is automatic detection.
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub cursor: Cursor,
    #[serde(default)]
    /// Top visible line (pixels mean nothing before allocation and shift with zoom).
    pub scroll_line: u32,
    #[serde(default)]
    pub selection: Option<Selection>,
}

impl TabRecord {
    pub fn new_untitled(title: &str) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            path: None,
            title: title.into(),
            pinned: false,
            dirty: false,
            buffer_file: None,
            base: None,
            encoding: "UTF-8".into(),
            bom: false,
            line_ending: Default::default(),
            language: None,
            cursor: Cursor::default(),
            scroll_line: 0,
            selection: None,
        }
    }

    /// Untitled tabs with text and dirty file tabs need their text stored; clean file tabs are re-read.
    pub fn needs_buffer(&self, has_text: bool) -> bool {
        if self.path.is_none() { has_text } else { self.dirty }
    }

    pub fn buffer_file_name(id: &str) -> String {
        format!("{id}.txt")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub version: u32,
    /// Seconds since the Unix epoch.
    pub saved_at: u64,
    #[serde(default)]
    pub active_tab: Option<String>,
    pub tabs: Vec<TabRecord>,
    #[serde(default)]
    pub closed_tabs: Vec<TabRecord>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            version: SESSION_VERSION,
            saved_at: 0,
            active_tab: None,
            tabs: Vec::new(),
            closed_tabs: Vec::new(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    Invalid(String),
    /// Written by a newer Notey. Read-only best effort; never overwrite.
    TooNew(u32),
}

impl Session {
    pub fn parse(json: &str) -> Result<Session, ParseError> {
        // Peek at the version first: a newer file may not match our struct at all.
        let value: serde_json::Value =
            serde_json::from_str(json).map_err(|e| ParseError::Invalid(e.to_string()))?;
        let version = value.get("version").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        if version > SESSION_VERSION {
            return Err(ParseError::TooNew(version));
        }
        serde_json::from_value(value).map_err(|e| ParseError::Invalid(e.to_string()))
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("session serialises")
    }

    /// Pushes a closed tab, newest first, bounded.
    pub fn remember_closed(&mut self, tab: TabRecord) {
        self.closed_tabs.insert(0, tab);
        self.closed_tabs.truncate(MAX_CLOSED_TABS);
    }

    /// Buffer files the manifest refers to.
    pub fn referenced_buffers(&self) -> std::collections::HashSet<&str> {
        self.tabs
            .iter()
            .chain(&self.closed_tabs)
            .filter_map(|t| t.buffer_file.as_deref())
            .collect()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Reconcile {
    /// Disk matches what the snapshot was based on.
    InSync,
    /// Disk changed since the snapshot: keep the user's buffer and offer Reload / Keep mine.
    DiskChanged,
    /// The file is gone.
    Missing,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Same size and either the same mtime or the same content: touching a file
/// without changing it does not count as a change.
pub fn reconcile(base: Option<&DiskState>, disk: Option<&DiskState>) -> Reconcile {
    match (base, disk) {
        (_, None) => Reconcile::Missing,
        (None, Some(_)) => Reconcile::DiskChanged,
        (Some(b), Some(d)) => {
            if b.size == d.size && (b.mtime_ns == d.mtime_ns || b.sha256 == d.sha256) {
                Reconcile::InSync
            } else {
                Reconcile::DiskChanged
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disk(mtime: i64, size: u64, hash: &str) -> DiskState {
        DiskState { mtime_ns: mtime, size, sha256: hash.into() }
    }

    #[test]
    fn round_trips() {
        let mut s = Session::default();
        let mut t = TabRecord::new_untitled("Untitled");
        t.buffer_file = Some(TabRecord::buffer_file_name(&t.id));
        t.cursor = Cursor { line: 4, column: 2 };
        t.selection = Some(Selection { start: Cursor { line: 1, column: 0 }, end: Cursor { line: 2, column: 3 } });
        s.active_tab = Some(t.id.clone());
        s.tabs.push(t);
        let parsed = Session::parse(&s.to_json()).unwrap();
        assert_eq!(parsed, s);
    }

    #[test]
    fn minimal_records_get_defaults() {
        let json = r#"{"version":1,"saved_at":5,"tabs":[{"id":"a","path":null,"title":"T","encoding":"UTF-8","line_ending":"crlf"}]}"#;
        let s = Session::parse(json).unwrap();
        assert_eq!(s.tabs[0].line_ending, crate::core::line_ending::LineEnding::Crlf);
        assert!(!s.tabs[0].dirty);
        assert!(s.closed_tabs.is_empty());
    }

    #[test]
    fn corrupt_and_newer_files_are_distinguished() {
        assert!(matches!(Session::parse("{not json"), Err(ParseError::Invalid(_))));
        assert!(matches!(Session::parse(r#"{"version":1}"#), Err(ParseError::Invalid(_))));
        assert_eq!(Session::parse(r#"{"version":99,"future":true}"#), Err(ParseError::TooNew(99)));
    }

    #[test]
    fn closed_tabs_are_bounded_newest_first() {
        let mut s = Session::default();
        for i in 0..25 {
            s.remember_closed(TabRecord::new_untitled(&format!("t{i}")));
        }
        assert_eq!(s.closed_tabs.len(), MAX_CLOSED_TABS);
        assert_eq!(s.closed_tabs[0].title, "t24");
    }

    #[test]
    fn buffer_rules() {
        let mut t = TabRecord::new_untitled("U");
        assert!(!t.needs_buffer(false));
        assert!(t.needs_buffer(true));
        t.path = Some("/x".into());
        assert!(!t.needs_buffer(true));
        t.dirty = true;
        assert!(t.needs_buffer(true));
    }

    #[test]
    fn referenced_buffers_include_closed_tabs() {
        let mut s = Session::default();
        let mut a = TabRecord::new_untitled("a");
        a.buffer_file = Some("a.txt".into());
        let mut b = TabRecord::new_untitled("b");
        b.buffer_file = Some("b.txt".into());
        s.tabs.push(a);
        s.remember_closed(b);
        assert_eq!(s.referenced_buffers().len(), 2);
    }

    #[test]
    fn reconciliation() {
        let base = disk(10, 5, "h");
        assert_eq!(reconcile(Some(&base), Some(&disk(10, 5, "h"))), Reconcile::InSync);
        assert_eq!(reconcile(Some(&base), Some(&disk(99, 5, "h"))), Reconcile::InSync, "touch only");
        assert_eq!(reconcile(Some(&base), Some(&disk(99, 5, "other"))), Reconcile::DiskChanged);
        assert_eq!(reconcile(Some(&base), Some(&disk(10, 6, "h"))), Reconcile::DiskChanged);
        assert_eq!(reconcile(Some(&base), None), Reconcile::Missing);
        assert_eq!(reconcile(None, Some(&base)), Reconcile::DiskChanged);
    }

    #[test]
    fn hash_is_stable() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
