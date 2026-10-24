use adw::prelude::*;
use notey::settings::Settings;
use notey::ui::window::{Notey, NoteyWindow, Options};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

pub struct Scenario {
    pub name: &'static str,
    pub run: fn(&Env),
}

pub fn scenario(name: &'static str, run: fn(&Env)) -> Scenario {
    Scenario { name, run }
}

thread_local! {
    static WARNINGS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Counts GLib, GTK and libadwaita warnings and criticals so a scenario can be failed on them.
pub fn install_warning_counter() {
    for domain in ["GLib", "Gtk", "Adwaita", "GtkSourceView", "GLib-GObject", "Gdk", "GLib-GIO"] {
        glib::log_set_handler(Some(domain), glib::LogLevels::LEVEL_CRITICAL | glib::LogLevels::LEVEL_WARNING, false, false, move |d, level, msg| {
            WARNINGS.with(|w| w.borrow_mut().push(format!("{} [{level:?}] {msg}", d.unwrap_or_default())));
        });
    }
}

pub fn reset_warnings() {
    WARNINGS.with(|w| w.borrow_mut().clear());
}

pub fn take_warnings() -> Vec<String> {
    WARNINGS.with(|w| std::mem::take(&mut *w.borrow_mut()))
}

pub struct Env {
    pub app: adw::Application,
    windows: RefCell<Vec<NoteyWindow>>,
}

impl Env {
    pub fn new() -> Env {
        let app = adw::Application::builder().application_id("com.zorin.NoteyTest").flags(gio::ApplicationFlags::NON_UNIQUE).build();
        app.register(gio::Cancellable::NONE).unwrap();
        Env { app, windows: RefCell::new(Vec::new()) }
    }

    pub fn close_all_windows(&self) {
        // Toasts still on screen when their window is destroyed keep firing GLib criticals.
        for w in self.windows.borrow_mut().drain(..) {
            w.dismiss_toasts();
        }
        for w in self.app.windows() {
            w.destroy();
        }
        pump_for(10);
    }

    /// A sandbox directory for files, plus a window on a session inside it.
    pub fn sandbox(&self) -> Sandbox {
        Sandbox { dir: tempfile::tempdir().unwrap() }
    }

    pub fn window(&self, sb: &Sandbox) -> NoteyWindow {
        self.window_with(sb, "session", Settings::default())
    }

    pub fn window_with(&self, sb: &Sandbox, session: &str, settings: Settings) -> NoteyWindow {
        let win = Notey::new(&self.app, Options { session_dir: sb.path(session), settings, persist_settings: false });
        self.windows.borrow_mut().push(win.clone());
        win
    }
}

pub struct Sandbox {
    dir: tempfile::TempDir,
}

impl Sandbox {
    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    pub fn write(&self, name: &str, contents: impl AsRef<[u8]>) -> PathBuf {
        let p = self.path(name);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&p, contents).unwrap();
        p
    }

    pub fn read(&self, name: &str) -> Vec<u8> {
        std::fs::read(self.path(name)).unwrap()
    }

    pub fn session_dir(&self) -> PathBuf {
        self.path("session")
    }

    pub fn buffer_file(&self, doc_id: &str) -> PathBuf {
        self.session_dir().join("buffers").join(format!("{doc_id}.txt"))
    }
}

pub fn pump_until(what: &str, mut cond: impl FnMut() -> bool) {
    let ctx = glib::MainContext::default();
    let start = Instant::now();
    while !cond() {
        assert!(start.elapsed() < Duration::from_secs(10), "timed out waiting for {what}");
        ctx.iteration(false);
        std::thread::sleep(Duration::from_millis(2));
    }
}

pub fn pump_for(ms: u64) {
    let ctx = glib::MainContext::default();
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(ms) {
        ctx.iteration(false);
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Runs an async block on the main loop until it finishes and returns its value.
pub fn block_on<T: 'static>(fut: impl std::future::Future<Output = T> + 'static) -> T {
    let slot: Rc<RefCell<Option<T>>> = Rc::new(RefCell::new(None));
    let s = slot.clone();
    glib::spawn_future_local(async move {
        let v = fut.await;
        *s.borrow_mut() = Some(v);
    });
    pump_until("async task", || slot.borrow().is_some());
    let v = slot.borrow_mut().take().unwrap();
    v
}

pub fn open(win: &NoteyWindow, path: &Path) -> Option<Rc<notey::ui::doc::Doc>> {
    let (w, p) = (win.clone(), path.to_path_buf());
    block_on(async move { w.open_path(p, None).await })
}

pub fn open_ok(win: &NoteyWindow, path: &Path) -> Rc<notey::ui::doc::Doc> {
    open(win, path).unwrap_or_else(|| panic!("{} did not open", path.display()))
}

pub fn save(win: &NoteyWindow, doc: &Rc<notey::ui::doc::Doc>) -> bool {
    let (w, d) = (win.clone(), doc.clone());
    block_on(async move { w.save_doc(&d, false).await })
}

/// Activates a window action as a keyboard shortcut or menu item would.
pub fn act(win: &NoteyWindow, name: &str) {
    assert!(gio::prelude::ActionMapExt::lookup_action(&win.window, name).is_some(), "no such action: {name}");
    gio::prelude::ActionGroupExt::activate_action(&win.window, name, None);
    pump_for(15);
}

pub fn select_all(doc: &notey::ui::doc::Doc) {
    doc.buffer.select_range(&doc.buffer.start_iter(), &doc.buffer.end_iter());
}

pub fn append(doc: &notey::ui::doc::Doc, text: &str) {
    doc.buffer.insert(&mut doc.buffer.end_iter(), text);
}

/// Force-closes a tab without a prompt, as "Discard" would.
pub fn discard_tab(win: &NoteyWindow, doc: &Rc<notey::ui::doc::Doc>) {
    doc.force_close.set(true);
    let page = win.page_of(doc).expect("doc is in a tab");
    win.tab_view.close_page(&page);
    pump_for(20);
}

/// Waits for the background writer to put the tab's unsaved text on disk (or take it away).
pub fn wait_for_buffer_file(sb: &Sandbox, doc_id: &str, present: bool) {
    let path = sb.buffer_file(doc_id);
    pump_until(if present { "the snapshot file to be written" } else { "the snapshot file to be removed" }, || path.exists() == present);
}

pub fn info_rows(doc: &notey::ui::doc::Doc) -> usize {
    let mut n = 0;
    let mut c = doc.info.first_child();
    while let Some(w) = c {
        n += 1;
        c = w.next_sibling();
    }
    n
}

pub fn page_title(win: &NoteyWindow, doc: &notey::ui::doc::Doc) -> String {
    win.page_of(doc).unwrap().title().to_string()
}

/// Simulates quitting: flush and tear the window down, as Ctrl+Q would.
pub fn quit(win: &NoteyWindow) {
    win.shutdown(false);
    win.window.destroy();
}

/// Marks an unused import as used for modules that only need part of the helpers.
#[allow(dead_code)]
pub fn unused(_: Cell<u8>) {}
