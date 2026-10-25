//! The pieces that must agree for the app to install, show an icon and open files: no display needed.
use std::fs;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(rel: &str) -> String {
    fs::read_to_string(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn desktop_key(key: &str) -> String {
    read("resources/com.zorin.Notey.desktop")
        .lines()
        .find_map(|l| l.strip_prefix(&format!("{key}=")).map(str::to_string))
        .unwrap_or_else(|| panic!("desktop file has no {key}="))
}

#[test]
fn the_app_id_is_the_same_everywhere() {
    let id = notey::ui::APP_ID;
    assert_eq!(id, "com.zorin.Notey");
    assert_eq!(desktop_key("Icon"), id);
    assert_eq!(desktop_key("StartupWMClass"), id);
    assert!(read("packaging/com.zorin.Notey.metainfo.xml").contains(&format!("<id>{id}</id>")));
    assert!(read("packaging/com.zorin.Notey.metainfo.xml").contains(&format!("{id}.desktop")));
    assert!(read("packaging/deb/build-deb.sh").contains(&format!("app_id=\"{id}\"")));
}

#[test]
fn the_icons_exist_and_are_well_formed_svg() {
    for rel in [
        "resources/icons/hicolor/scalable/apps/com.zorin.Notey.svg",
        "resources/icons/hicolor/symbolic/apps/com.zorin.Notey-symbolic.svg",
    ] {
        let svg = read(rel);
        assert!(svg.trim_start().starts_with("<?xml") && svg.trim_end().ends_with("</svg>"), "{rel}");
        assert!(svg.contains("viewBox=") && svg.contains("xmlns=\"http://www.w3.org/2000/svg\""), "{rel}");
        // Every opened element is closed: a cheap well-formedness check without an XML crate.
        let opens = svg.matches('<').count();
        assert_eq!(svg.matches('>').count(), opens, "{rel}: unbalanced angle brackets");
    }
    let colour = read("resources/icons/hicolor/scalable/apps/com.zorin.Notey.svg");
    assert!(colour.contains("viewBox=\"0 0 128 128\""));
    let symbolic = read("resources/icons/hicolor/symbolic/apps/com.zorin.Notey-symbolic.svg");
    assert!(symbolic.contains("viewBox=\"0 0 16 16\""), "symbolic icons are 16x16");
}

#[test]
fn the_package_installs_both_icons_the_desktop_file_and_the_metainfo() {
    let script = read("packaging/deb/build-deb.sh");
    for needle in ["usr/share/icons/hicolor/scalable/apps", "usr/share/icons/hicolor/symbolic/apps", "usr/share/applications", "usr/share/metainfo"] {
        assert!(script.contains(needle), "build-deb.sh does not install into {needle}");
    }
}

#[test]
fn the_desktop_file_can_open_files_and_names_text_types() {
    let exec = desktop_key("Exec");
    assert!(exec.starts_with("notey") && (exec.contains("%F") || exec.contains("%U")), "Exec must take files: {exec}");
    assert_eq!(desktop_key("Type"), "Application");
    assert_eq!(desktop_key("Terminal"), "false");
    let mimes = desktop_key("MimeType");
    for m in ["text/plain", "text/markdown", "application/json", "text/x-python", "text/x-csrc", "application/x-shellscript"] {
        assert!(mimes.split(';').any(|x| x == m), "{m} missing from MimeType");
    }
    assert!(mimes.ends_with(';'), "MimeType must end with a semicolon");
    assert!(desktop_key("Categories").contains("TextEditor"));
}

#[test]
fn the_metainfo_release_matches_the_crate_version() {
    let version = env!("CARGO_PKG_VERSION");
    assert!(read("packaging/com.zorin.Notey.metainfo.xml").contains(&format!("<release version=\"{version}\"")), "add a <release> for {version}");
}

#[test]
fn every_file_type_offered_as_a_default_is_in_the_desktop_file() {
    let mimes = desktop_key("MimeType");
    let source = read("src/ui/preferences.rs");
    let table = source.split("const FILE_TYPE_GROUPS").nth(1).and_then(|t| t.split("];").next()).expect("FILE_TYPE_GROUPS");
    for m in table.split('"').skip(1).step_by(2).filter(|s| s.contains('/')) {
        assert!(mimes.split(';').any(|x| x == m), "{m} is offered in Preferences but missing from MimeType");
    }
}
