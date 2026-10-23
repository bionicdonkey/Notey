use adw::prelude::*;
use gio::ApplicationFlags;
use notey::ui::window::{Notey, NoteyWindow, Options};
use std::cell::RefCell;
use std::rc::Rc;

fn main() {
    if std::env::args().skip(1).any(|a| a == "--version" || a == "-V") {
        println!("notey {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    // Normally one instance serves every `notey` launch. A throwaway profile (tests, trying a
    // build next to the installed one) must not hand its files to that instance.
    let mut flags = ApplicationFlags::HANDLES_COMMAND_LINE;
    if std::env::var_os("NOTEY_NON_UNIQUE").is_some() {
        flags |= ApplicationFlags::NON_UNIQUE;
    }
    let app = adw::Application::builder().application_id(notey::ui::APP_ID).flags(flags).build();
    let window: Rc<RefCell<Option<NoteyWindow>>> = Rc::new(RefCell::new(None));

    app.connect_startup(|_| {
        sourceview5::init();
        notey::ui::install_css();
        notey::ui::install_icons();
        notey::ui::install_accels();
    });
    app.connect_command_line({
        let window = window.clone();
        move |app, cmdline| {
            let existing = window.borrow().clone();
            let win = existing.unwrap_or_else(|| {
                let w = Notey::new(app, Options::from_environment());
                *window.borrow_mut() = Some(w.clone());
                w
            });
            let files = notey::cli::parse_args(&cmdline.arguments(), cmdline.cwd());
            if files.is_empty() {
                win.window.present();
            } else {
                for (path, line) in files {
                    win.open_in_background(vec![path], line);
                }
            }
            0
        }
    });
    app.run();
}
