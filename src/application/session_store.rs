//! Crash-safe storage for the session manifest and per-tab buffer files.
use super::fs_util::atomic_write;
use crate::core::session::{ParseError, Session};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const MANIFEST: &str = "session.json";
const BACKUP: &str = "session.json.bak";
const BUFFERS: &str = "buffers";

/// `NOTEY_SESSION_DIR`, else `$XDG_DATA_HOME/notey/session`, else `~/.local/share/notey/session`.
pub fn default_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("NOTEY_SESSION_DIR") {
        return d.into();
    }
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("notey/session")
}

#[derive(Debug, PartialEq, Eq)]
pub enum Source {
    Fresh,
    Manifest,
    /// The manifest was unreadable; the previous one was used.
    Backup,
    /// Both were unreadable; they were moved aside and we start empty.
    Quarantined(PathBuf),
    /// Written by a newer Notey. Loaded read-only and never overwritten.
    Newer(u32),
}

pub struct Loaded {
    pub session: Session,
    pub source: Source,
}

pub struct SessionStore {
    dir: PathBuf,
    /// Set when the on-disk session came from a newer version.
    frozen: bool,
}

#[cfg(unix)]
fn private_dir(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    fs::DirBuilder::new().recursive(true).mode(0o700).create(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn private_dir(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)
}

impl SessionStore {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, frozen: false }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn buffers_dir(&self) -> PathBuf {
        self.dir.join(BUFFERS)
    }

    fn parse_file(path: &Path) -> Result<Session, ParseError> {
        let text = fs::read_to_string(path).map_err(|e| ParseError::Invalid(e.to_string()))?;
        Session::parse(&text)
    }

    pub fn load(&mut self) -> Loaded {
        let manifest = self.dir.join(MANIFEST);
        let backup = self.dir.join(BACKUP);
        if !manifest.exists() && !backup.exists() {
            return Loaded { session: Session::default(), source: Source::Fresh };
        }
        match Self::parse_file(&manifest) {
            Ok(session) => return Loaded { session, source: Source::Manifest },
            Err(ParseError::TooNew(v)) => {
                self.frozen = true;
                return Loaded { session: Session::default(), source: Source::Newer(v) };
            }
            Err(ParseError::Invalid(_)) => {}
        }
        if let Ok(session) = Self::parse_file(&backup) {
            return Loaded { session, source: Source::Backup };
        }
        let aside = self.quarantine();
        Loaded { session: Session::default(), source: Source::Quarantined(aside) }
    }

    /// Moves unreadable manifests (not the buffer files, which may still hold text) aside.
    fn quarantine(&self) -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let aside = self.dir.join(format!("corrupt-{stamp}"));
        let _ = private_dir(&aside);
        for name in [MANIFEST, BACKUP] {
            let from = self.dir.join(name);
            if from.exists() {
                let _ = fs::rename(&from, aside.join(name));
            }
        }
        aside
    }

    /// Writes the manifest, keeping the previous one as `session.json.bak`.
    pub fn save_manifest(&self, session: &Session) -> io::Result<()> {
        if self.frozen {
            return Ok(());
        }
        private_dir(&self.dir)?;
        let manifest = self.dir.join(MANIFEST);
        if manifest.exists() {
            // A copy, not a rename: the manifest must exist at every instant.
            fs::copy(&manifest, self.dir.join(BACKUP))?;
        }
        atomic_write(&manifest, session.to_json().as_bytes(), Some(0o600))
    }

    pub fn write_buffer(&self, file: &str, text: &str) -> io::Result<()> {
        if self.frozen {
            return Ok(());
        }
        private_dir(&self.buffers_dir())?;
        atomic_write(&self.buffers_dir().join(file), text.as_bytes(), Some(0o600))
    }

    pub fn read_buffer(&self, file: &str) -> io::Result<String> {
        // Names come from the manifest; refuse anything that is not a plain file name.
        if Path::new(file).components().count() != 1 || file.contains("..") {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "bad buffer name"));
        }
        fs::read_to_string(self.buffers_dir().join(file))
    }

    pub fn remove_buffer(&self, file: &str) {
        let _ = fs::remove_file(self.buffers_dir().join(file));
    }

    /// Deletes buffer files the session no longer refers to. Returns how many.
    pub fn remove_orphans(&self, session: &Session) -> usize {
        if self.frozen {
            return 0;
        }
        let keep = session.referenced_buffers();
        let Ok(rd) = fs::read_dir(self.buffers_dir()) else { return 0 };
        let mut removed = 0;
        for entry in rd.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            // Leftover temp files from an interrupted write are orphans too.
            if !keep.contains(name.as_ref()) && fs::remove_file(entry.path()).is_ok() {
                removed += 1;
            }
        }
        removed
    }

    /// Forget everything ("Remember tabs" off, or "Clear saved session").
    pub fn clear(&self) {
        if self.frozen {
            return;
        }
        let _ = fs::remove_dir_all(self.buffers_dir());
        let _ = fs::remove_file(self.dir.join(MANIFEST));
        let _ = fs::remove_file(self.dir.join(BACKUP));
    }

    /// Loads the text of every tab that has a buffer file. A missing or unreadable
    /// file maps to `None` so one bad tab never blocks the others.
    pub fn read_all_buffers(&self, session: &Session) -> HashMap<String, Option<String>> {
        session
            .tabs
            .iter()
            .chain(&session.closed_tabs)
            .filter_map(|t| t.buffer_file.as_ref().map(|f| (t.id.clone(), self.read_buffer(f).ok())))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::session::TabRecord;

    fn store() -> (tempfile::TempDir, SessionStore) {
        let d = tempfile::tempdir().unwrap();
        let s = SessionStore::new(d.path().join("session"));
        (d, s)
    }

    fn tab_with_buffer(title: &str) -> TabRecord {
        let mut t = TabRecord::new_untitled(title);
        t.buffer_file = Some(TabRecord::buffer_file_name(&t.id));
        t
    }

    #[test]
    fn empty_directory_is_a_fresh_start() {
        let (_d, mut s) = store();
        let l = s.load();
        assert_eq!(l.source, Source::Fresh);
        assert!(l.session.tabs.is_empty());
    }

    #[test]
    fn manifest_and_buffers_round_trip() {
        let (_d, mut s) = store();
        let t = tab_with_buffer("Untitled");
        s.write_buffer(t.buffer_file.as_ref().unwrap(), "unsaved text\n").unwrap();
        let mut session = Session::default();
        session.tabs.push(t.clone());
        s.save_manifest(&session).unwrap();

        let l = s.load();
        assert_eq!(l.source, Source::Manifest);
        assert_eq!(l.session, session);
        assert_eq!(s.read_buffer(&t.buffer_file.unwrap()).unwrap(), "unsaved text\n");
    }

    #[test]
    fn second_save_keeps_the_previous_manifest_as_backup() {
        let (_d, mut s) = store();
        let mut a = Session::default();
        a.tabs.push(TabRecord::new_untitled("first"));
        s.save_manifest(&a).unwrap();
        let mut b = Session::default();
        b.tabs.push(TabRecord::new_untitled("second"));
        s.save_manifest(&b).unwrap();
        let bak = Session::parse(&fs::read_to_string(s.dir().join(BACKUP)).unwrap()).unwrap();
        assert_eq!(bak.tabs[0].title, "first");
        assert_eq!(s.load().session.tabs[0].title, "second");
    }

    #[test]
    fn corrupt_manifest_falls_back_to_backup() {
        let (_d, mut s) = store();
        let mut a = Session::default();
        a.tabs.push(TabRecord::new_untitled("kept"));
        s.save_manifest(&a).unwrap();
        s.save_manifest(&a).unwrap(); // now a .bak exists
        fs::write(s.dir().join(MANIFEST), "{ truncated").unwrap();
        let l = s.load();
        assert_eq!(l.source, Source::Backup);
        assert_eq!(l.session.tabs[0].title, "kept");
    }

    #[test]
    fn both_corrupt_quarantines_and_starts_empty() {
        let (_d, mut s) = store();
        fs::create_dir_all(s.dir()).unwrap();
        fs::write(s.dir().join(MANIFEST), "garbage").unwrap();
        fs::write(s.dir().join(BACKUP), "garbage").unwrap();
        let l = s.load();
        let Source::Quarantined(aside) = l.source else { panic!("expected quarantine") };
        assert!(aside.join(MANIFEST).exists());
        assert!(!s.dir().join(MANIFEST).exists());
        assert!(l.session.tabs.is_empty());
    }

    #[test]
    fn newer_version_is_never_overwritten() {
        let (_d, mut s) = store();
        fs::create_dir_all(s.dir()).unwrap();
        let future = r#"{"version":99,"tabs":[]}"#;
        fs::write(s.dir().join(MANIFEST), future).unwrap();
        assert_eq!(s.load().source, Source::Newer(99));
        s.save_manifest(&Session::default()).unwrap();
        s.write_buffer("x.txt", "y").unwrap();
        assert_eq!(fs::read_to_string(s.dir().join(MANIFEST)).unwrap(), future);
        assert!(!s.buffers_dir().join("x.txt").exists());
    }

    #[test]
    fn one_missing_buffer_does_not_block_the_rest() {
        let (_d, s) = store();
        let (a, b) = (tab_with_buffer("a"), tab_with_buffer("b"));
        s.write_buffer(a.buffer_file.as_ref().unwrap(), "A").unwrap();
        let session = Session { tabs: vec![a.clone(), b.clone()], ..Session::default() };
        let all = s.read_all_buffers(&session);
        assert_eq!(all[&a.id].as_deref(), Some("A"));
        assert_eq!(all[&b.id], None);
    }

    #[test]
    fn orphans_are_removed_but_referenced_buffers_survive() {
        let (_d, s) = store();
        let keep = tab_with_buffer("keep");
        let closed = tab_with_buffer("closed");
        s.write_buffer(keep.buffer_file.as_ref().unwrap(), "k").unwrap();
        s.write_buffer(closed.buffer_file.as_ref().unwrap(), "c").unwrap();
        s.write_buffer("stale.txt", "x").unwrap();
        fs::write(s.buffers_dir().join(".half.notey-tmp"), "x").unwrap();
        let mut session = Session::default();
        session.tabs.push(keep.clone());
        session.remember_closed(closed.clone());
        assert_eq!(s.remove_orphans(&session), 2);
        assert!(s.read_buffer(keep.buffer_file.as_ref().unwrap()).is_ok());
        assert!(s.read_buffer(closed.buffer_file.as_ref().unwrap()).is_ok());
    }

    #[test]
    fn crash_between_first_edit_and_save_never_loses_text() {
        // Sequence the UI performs: buffer first, then a manifest that references it.
        let (_d, s) = store();
        let mut t = tab_with_buffer("Untitled");
        s.write_buffer(t.buffer_file.as_ref().unwrap(), "typed").unwrap();
        t.dirty = true;
        let mut session = Session::default();
        session.tabs.push(t.clone());
        s.save_manifest(&session).unwrap();
        // "Crash" and relaunch: the text must be there and survive orphan cleanup.
        let mut relaunched = SessionStore::new(s.dir().to_path_buf());
        let l = relaunched.load();
        assert_eq!(relaunched.remove_orphans(&l.session), 0);
        assert_eq!(relaunched.read_buffer(t.buffer_file.as_ref().unwrap()).unwrap(), "typed");

        // Saving: manifest without the buffer goes first, only then the buffer is deleted.
        let mut saved = t.clone();
        saved.dirty = false;
        saved.buffer_file = None;
        session.tabs = vec![saved];
        relaunched.save_manifest(&session).unwrap();
        relaunched.remove_buffer(t.buffer_file.as_ref().unwrap());
        assert!(relaunched.read_buffer(t.buffer_file.as_ref().unwrap()).is_err());
        assert_eq!(relaunched.load().session.tabs[0].buffer_file, None);
    }

    #[test]
    fn buffer_names_cannot_escape_the_directory() {
        let (_d, s) = store();
        assert!(s.read_buffer("../session.json").is_err());
        assert!(s.read_buffer("/etc/passwd").is_err());
        assert!(s.read_buffer("a/b").is_err());
    }

    #[test]
    fn clear_forgets_everything() {
        let (_d, mut s) = store();
        s.write_buffer("a.txt", "x").unwrap();
        s.save_manifest(&Session::default()).unwrap();
        s.clear();
        assert_eq!(s.load().source, Source::Fresh);
        assert!(s.read_buffer("a.txt").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn files_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let (_d, s) = store();
        s.write_buffer("a.txt", "x").unwrap();
        s.save_manifest(&Session::default()).unwrap();
        let mode = |p: PathBuf| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(s.dir().to_path_buf()), 0o700);
        assert_eq!(mode(s.dir().join(MANIFEST)), 0o600);
        assert_eq!(mode(s.buffers_dir().join("a.txt")), 0o600);
    }
}

enum Job {
    Buffer { file: String, text: String },
    RemoveBuffer(String),
    Manifest(Session),
    /// Delete every buffer file the given session does not refer to. Queued after the manifest that
    /// session came from, so what the stored manifest points at is never removed.
    Prune(Session),
}

impl Job {
    fn key(&self) -> String {
        match self {
            Job::Buffer { file, .. } | Job::RemoveBuffer(file) => format!("buf:{file}"),
            Job::Manifest(_) => "manifest".into(),
            Job::Prune(_) => "prune".into(),
        }
    }
}

enum Msg {
    Job(Job),
    /// Run everything pending, then reply.
    Flush(std::sync::mpsc::Sender<Vec<String>>),
    Stop,
}

/// The only writer of session files. Jobs are queued from the UI thread; a newer job
/// for the same key replaces the older one *and moves to the back of the queue*, so a
/// manifest that drops a buffer always lands before that buffer's deletion.
pub struct SessionWriter {
    tx: std::sync::mpsc::Sender<Msg>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl SessionWriter {
    pub fn start(store: SessionStore) -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<Msg>();
        let handle = std::thread::Builder::new()
            .name("notey-session-writer".into())
            .spawn(move || {
                let mut pending: Vec<(String, Job)> = Vec::new();
                let mut errors: Vec<String> = Vec::new();
                let mut running = true;
                while running {
                    let Ok(first) = rx.recv() else { break };
                    let mut replies = Vec::new();
                    let mut batch = vec![first];
                    batch.extend(rx.try_iter());
                    for msg in batch {
                        match msg {
                            Msg::Job(job) => {
                                let key = job.key();
                                pending.retain(|(k, _)| *k != key);
                                pending.push((key, job));
                            }
                            Msg::Flush(reply) => replies.push(reply),
                            Msg::Stop => running = false,
                        }
                    }
                    for (_, job) in pending.drain(..) {
                        let r = match &job {
                            Job::Buffer { file, text } => store.write_buffer(file, text),
                            Job::RemoveBuffer(file) => {
                                store.remove_buffer(file);
                                Ok(())
                            }
                            Job::Manifest(s) => store.save_manifest(s),
                            Job::Prune(s) => {
                                store.remove_orphans(s);
                                Ok(())
                            }
                        };
                        if let Err(e) = r {
                            errors.push(e.to_string());
                        }
                    }
                    for reply in replies {
                        let _ = reply.send(std::mem::take(&mut errors));
                    }
                }
            })
            .expect("spawn session writer");
        Self { tx, handle: Some(handle) }
    }

    pub fn write_buffer(&self, file: String, text: String) {
        let _ = self.tx.send(Msg::Job(Job::Buffer { file, text }));
    }

    pub fn remove_buffer(&self, file: String) {
        let _ = self.tx.send(Msg::Job(Job::RemoveBuffer(file)));
    }

    pub fn write_manifest(&self, session: Session) {
        let _ = self.tx.send(Msg::Job(Job::Manifest(session)));
    }

    /// Deletes every stored buffer file `session` does not refer to (closed tabs' text, leftovers).
    /// Queue the manifest for the same session first: files the stored manifest points at must
    /// never disappear, not even for an instant.
    pub fn prune(&self, session: Session) {
        let _ = self.tx.send(Msg::Job(Job::Prune(session)));
    }

    /// Blocks until everything queued so far is on disk. Returns write errors since the last flush.
    pub fn flush(&self) -> Vec<String> {
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        if self.tx.send(Msg::Flush(reply_tx)).is_err() {
            return Vec::new();
        }
        reply_rx.recv().unwrap_or_default()
    }

    /// Flushes and stops the thread. Later calls are silently ignored, so teardown
    /// handlers that fire afterwards cannot write a half-empty session.
    pub fn shutdown(&mut self) -> Vec<String> {
        let errors = self.flush();
        let _ = self.tx.send(Msg::Stop);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        errors
    }
}

impl Drop for SessionWriter {
    fn drop(&mut self) {
        if self.handle.is_some() {
            self.shutdown();
        }
    }
}

#[cfg(test)]
mod writer_tests {
    use super::*;
    use crate::core::session::TabRecord;

    #[test]
    fn jobs_are_applied_and_flush_waits_for_them() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("s");
        let w = SessionWriter::start(SessionStore::new(dir.clone()));
        w.write_buffer("a.txt".into(), "one".into());
        w.write_buffer("a.txt".into(), "two".into());
        assert!(w.flush().is_empty());
        assert_eq!(fs::read_to_string(dir.join("buffers/a.txt")).unwrap(), "two");
    }

    #[test]
    fn removal_queued_after_a_manifest_lands_after_it() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("s");
        let w = SessionWriter::start(SessionStore::new(dir.clone()));
        let mut t = TabRecord::new_untitled("U");
        t.buffer_file = Some("a.txt".into());
        let mut with = Session::default();
        with.tabs.push(t.clone());
        w.write_buffer("a.txt".into(), "text".into());
        w.write_manifest(with);
        let mut without = Session::default();
        t.buffer_file = None;
        without.tabs.push(t);
        w.write_manifest(without);
        w.remove_buffer("a.txt".into());
        assert!(w.flush().is_empty());
        assert!(!dir.join("buffers/a.txt").exists());
        let m = Session::parse(&fs::read_to_string(dir.join("session.json")).unwrap()).unwrap();
        assert_eq!(m.tabs[0].buffer_file, None);
    }

    #[test]
    fn prune_removes_only_what_the_session_no_longer_refers_to() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("s");
        let w = SessionWriter::start(SessionStore::new(dir.clone()));
        let mut open = TabRecord::new_untitled("open");
        open.buffer_file = Some("open.txt".into());
        let mut keep = Session::default();
        keep.tabs.push(open);
        w.write_buffer("open.txt".into(), "still needed".into());
        w.write_buffer("closed.txt".into(), "history".into());
        w.write_manifest(keep.clone());
        w.prune(keep);
        assert!(w.flush().is_empty());
        assert_eq!(fs::read_to_string(dir.join("buffers/open.txt")).unwrap(), "still needed");
        assert!(!dir.join("buffers/closed.txt").exists(), "unreferenced copy is gone");
        assert!(dir.join("session.json").exists(), "the manifest is never removed");
    }

    #[test]
    fn nothing_is_written_after_shutdown() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("s");
        let mut w = SessionWriter::start(SessionStore::new(dir.clone()));
        w.write_buffer("a.txt".into(), "kept".into());
        w.shutdown();
        w.write_manifest(Session::default()); // e.g. a page-detached handler during teardown
        w.write_buffer("a.txt".into(), "late".into());
        assert_eq!(fs::read_to_string(dir.join("buffers/a.txt")).unwrap(), "kept");
        assert!(!dir.join("session.json").exists());
    }

    #[test]
    fn write_errors_are_reported_by_flush() {
        let d = tempfile::tempdir().unwrap();
        let blocker = d.path().join("blocker");
        fs::write(&blocker, "a file where the directory should be").unwrap();
        let w = SessionWriter::start(SessionStore::new(blocker.join("s")));
        w.write_buffer("a.txt".into(), "x".into());
        assert!(!w.flush().is_empty());
    }
}
