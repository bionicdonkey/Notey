//! Runs the real binary: startup, session persistence across a hard kill, and clean shutdown.
//! Needs a display (`xvfb-run -a cargo test`); skipped without one unless NOTEY_REQUIRE_DISPLAY=1.
use notey::core::session::Session;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn has_display() -> bool {
    std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

/// `false` means "skip this test".
fn display_or_skip() -> bool {
    if has_display() {
        return true;
    }
    assert!(std::env::var_os("NOTEY_REQUIRE_DISPLAY").is_none(), "NOTEY_REQUIRE_DISPLAY is set but there is no display");
    eprintln!("no display; skipping");
    false
}

struct Profile {
    _dir: tempfile::TempDir,
    session: std::path::PathBuf,
    config: std::path::PathBuf,
}

impl Profile {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let (session, config) = (dir.path().join("session"), dir.path().join("config"));
        Profile { _dir: dir, session, config }
    }

    fn path(&self, name: &str) -> std::path::PathBuf {
        self._dir.path().join(name)
    }

    fn spawn(&self, args: &[&Path]) -> Child {
        Command::new(env!("CARGO_BIN_EXE_notey"))
            .args(args)
            .env("NOTEY_SESSION_DIR", &self.session)
            .env("NOTEY_CONFIG_DIR", &self.config)
            .env("NOTEY_NON_UNIQUE", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn notey")
    }

    fn manifest(&self) -> Option<Session> {
        Session::parse(&std::fs::read_to_string(self.session.join("session.json")).ok()?).ok()
    }

    fn wait_for(&self, what: &str, cond: impl Fn(&Session) -> bool) -> Session {
        let start = Instant::now();
        loop {
            if let Some(s) = self.manifest() {
                if cond(&s) {
                    return s;
                }
            }
            assert!(start.elapsed() < Duration::from_secs(15), "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

fn signal(child: &Child, sig: i32) {
    unsafe { libc::kill(child.id() as i32, sig) };
}

fn paths(s: &Session) -> Vec<String> {
    s.tabs.iter().filter_map(|t| t.path.clone()).collect()
}

#[test]
fn version_flag_prints_and_exits_without_a_display() {
    let out = Command::new(env!("CARGO_BIN_EXE_notey")).arg("--version").env_remove("DISPLAY").env_remove("WAYLAND_DISPLAY").output().unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), format!("notey {}", env!("CARGO_PKG_VERSION")));
}

#[test]
fn files_on_the_command_line_open_and_survive_a_hard_kill() {
    if !display_or_skip() {
        return;
    }
    let p = Profile::new();
    let (a, b) = (p.path("a.txt"), p.path("b.rs"));
    std::fs::write(&a, "alpha\n").unwrap();
    std::fs::write(&b, "fn main() {}\n").unwrap();

    let mut first = p.spawn(&[&a, &b]);
    let canon = |f: &Path| std::fs::canonicalize(f).unwrap().to_string_lossy().into_owned();
    let s = p.wait_for("both files in the session", |s| paths(s).len() == 2);
    assert_eq!(paths(&s), vec![canon(&a), canon(&b)]);

    signal(&first, libc::SIGKILL);
    let status = first.wait().unwrap();
    assert!(!status.success(), "SIGKILL is not a clean exit");

    // A fresh process restores both tabs from what the dead one left behind.
    let mut second = p.spawn(&[]);
    std::thread::sleep(Duration::from_secs(3));
    assert!(second.try_wait().unwrap().is_none(), "restored instance is still running");
    let restored = p.manifest().unwrap();
    assert_eq!(paths(&restored), vec![canon(&a), canon(&b)], "tabs came back and were re-saved");

    signal(&second, libc::SIGTERM);
    let status = second.wait().unwrap();
    assert!(status.success(), "SIGTERM shuts down cleanly: {status:?}");
    assert_eq!(paths(&p.manifest().unwrap()), vec![canon(&a), canon(&b)], "graceful exit keeps the session");
    assert_eq!(std::fs::read_to_string(&a).unwrap(), "alpha\n", "files are never rewritten by the session");
}

#[test]
fn a_line_number_argument_is_accepted_and_the_file_opens() {
    if !display_or_skip() {
        return;
    }
    let p = Profile::new();
    let f = p.path("long.txt");
    std::fs::write(&f, (1..=200).map(|n| format!("line {n}\n")).collect::<String>()).unwrap();
    let target = format!("{}:150", f.display());
    let mut child = p.spawn(&[Path::new(&target)]);
    let s = p.wait_for("file in the session", |s| paths(s).len() == 1);
    assert!(paths(&s)[0].ends_with("long.txt"));
    // The cursor line is saved by the periodic manifest write; the first write may predate it.
    let s = p.wait_for("cursor on line 150", |s| s.tabs[0].cursor.line == 149);
    assert_eq!(s.tabs[0].cursor.line, 149);
    signal(&child, libc::SIGTERM);
    child.wait().unwrap();
}

#[test]
fn a_session_from_a_newer_version_is_left_alone() {
    if !display_or_skip() {
        return;
    }
    let p = Profile::new();
    std::fs::create_dir_all(&p.session).unwrap();
    let future = r#"{"version":99,"tabs":[],"from":"the future"}"#;
    std::fs::write(p.session.join("session.json"), future).unwrap();
    let mut child = p.spawn(&[]);
    std::thread::sleep(Duration::from_secs(3));
    signal(&child, libc::SIGTERM);
    child.wait().unwrap();
    assert_eq!(std::fs::read_to_string(p.session.join("session.json")).unwrap(), future);
}

#[test]
fn a_corrupt_session_is_set_aside_and_the_app_starts() {
    if !display_or_skip() {
        return;
    }
    let p = Profile::new();
    std::fs::create_dir_all(&p.session).unwrap();
    std::fs::write(p.session.join("session.json"), "{ not json").unwrap();
    let mut child = p.spawn(&[]);
    let s = p.wait_for("a fresh session", |s| s.tabs.len() == 1);
    assert!(s.tabs[0].path.is_none());
    let aside = std::fs::read_dir(&p.session).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with("corrupt-"));
    assert!(aside, "the unreadable manifest was kept in a corrupt-* folder");
    signal(&child, libc::SIGTERM);
    child.wait().unwrap();
}
