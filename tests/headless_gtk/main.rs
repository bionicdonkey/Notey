//! Window-level scenarios. GTK may only be initialised once per process, so this binary has its
//! own `main` (no libtest harness) and runs named scenarios one after another, each in its own
//! throwaway profile.
//!
//!   xvfb-run -a cargo test --test headless_gtk                 run everything
//!   xvfb-run -a cargo test --test headless_gtk -- find         only scenarios whose name contains "find"
//!   NOTEY_REQUIRE_DISPLAY=1 ...                                a missing display is a failure, not a skip
//!   NOTEY_STRICT_WARNINGS=1 ...                                GLib/GTK criticals fail the scenario
mod editing;
mod files;
mod find;
mod session;
mod support;
mod ui;

use std::panic::{catch_unwind, AssertUnwindSafe};
use support::{Env, Scenario};

fn main() {
    let filters: Vec<String> = std::env::args().skip(1).filter(|a| !a.starts_with("--")).collect();
    if adw::init().is_err() {
        if std::env::var_os("NOTEY_REQUIRE_DISPLAY").is_some() {
            eprintln!("NOTEY_REQUIRE_DISPLAY is set but GTK could not open a display");
            std::process::exit(1);
        }
        eprintln!("no display; skipping the window scenarios");
        return;
    }
    sourceview5::init();
    support::install_warning_counter();
    let env = Env::new();

    let mut all: Vec<Scenario> = Vec::new();
    all.extend(session::scenarios());
    all.extend(files::scenarios());
    all.extend(editing::scenarios());
    all.extend(find::scenarios());
    all.extend(ui::scenarios());

    let selected: Vec<&Scenario> = all.iter().filter(|s| filters.is_empty() || filters.iter().any(|f| s.name.contains(f.as_str()))).collect();
    let strict = std::env::var_os("NOTEY_STRICT_WARNINGS").is_some();
    let mut failed = Vec::new();
    println!("running {} window scenarios", selected.len());
    for s in &selected {
        support::reset_warnings();
        let result = catch_unwind(AssertUnwindSafe(|| (s.run)(&env)));
        env.close_all_windows();
        let warnings = support::take_warnings();
        match (&result, strict && !warnings.is_empty()) {
            (Ok(()), false) => println!("scenario {} ... ok", s.name),
            (Ok(()), true) => {
                println!("scenario {} ... FAILED (warnings)", s.name);
                for w in &warnings {
                    println!("    {w}");
                }
                failed.push(s.name);
            }
            (Err(e), _) => {
                let msg = e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
                println!("scenario {} ... FAILED\n    {}", s.name, msg.replace('\n', "\n    "));
                failed.push(s.name);
            }
        }
    }
    if failed.is_empty() {
        println!("\nwindow scenarios: {} passed", selected.len());
    } else {
        println!("\nwindow scenarios: {} failed of {}: {}", failed.len(), selected.len(), failed.join(", "));
        std::process::exit(1);
    }
}
