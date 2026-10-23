//! Command-line arguments: `FILE`, `FILE:LINE` and `+LINE FILE`.
use std::path::PathBuf;

/// Resolves relative paths against the caller's directory (`cwd`); the primary instance's own
/// working directory is unrelated. `args[0]` is the program name.
pub fn parse_args(args: &[std::ffi::OsString], cwd: Option<PathBuf>) -> Vec<(PathBuf, Option<u32>)> {
    let mut out = Vec::new();
    let mut pending_line = None;
    for a in args.iter().skip(1) {
        let s = a.to_string_lossy().into_owned();
        if let Some(n) = s.strip_prefix('+').and_then(|n| n.parse::<u32>().ok()) {
            pending_line = Some(n);
            continue;
        }
        let resolve = |p: &str| match &cwd {
            Some(c) if !PathBuf::from(p).is_absolute() => c.join(p),
            _ => PathBuf::from(p),
        };
        let mut path = resolve(&s);
        let mut line = pending_line.take();
        if !path.exists() {
            if let Some((file, n)) = s.rsplit_once(':') {
                if let Ok(n) = n.parse::<u32>() {
                    let candidate = resolve(file);
                    if candidate.exists() {
                        path = candidate;
                        line = Some(n);
                    }
                }
            }
        }
        out.push((path, line));
    }
    out
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn plain_files_resolve_against_the_callers_directory() {
        let got = parse_args(&args(&["notey", "a.txt", "/abs/b.txt"]), Some("/home/u".into()));
        assert_eq!(got, vec![("/home/u/a.txt".into(), None), ("/abs/b.txt".into(), None)]);
    }

    #[test]
    fn plus_line_applies_to_the_next_file_only() {
        let got = parse_args(&args(&["notey", "+42", "a.txt", "b.txt"]), Some("/w".into()));
        assert_eq!(got[0], (PathBuf::from("/w/a.txt"), Some(42)));
        assert_eq!(got[1], (PathBuf::from("/w/b.txt"), None));
    }

    #[test]
    fn file_colon_line_needs_the_file_to_exist() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("x.rs"), "").unwrap();
        let got = parse_args(&args(&["notey", "x.rs:7", "ghost.rs:9"]), Some(dir.path().into()));
        assert_eq!(got[0], (dir.path().join("x.rs"), Some(7)));
        // A name that merely looks like FILE:LINE stays a literal file name when FILE is missing.
        assert_eq!(got[1], (dir.path().join("ghost.rs:9"), None));
    }

    #[test]
    fn a_file_literally_named_with_a_colon_wins() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("odd:3"), "").unwrap();
        let got = parse_args(&args(&["notey", "odd:3"]), Some(dir.path().into()));
        assert_eq!(got, vec![(dir.path().join("odd:3"), None)]);
    }

    #[test]
    fn no_arguments_means_no_files() {
        assert!(parse_args(&args(&["notey"]), None).is_empty());
        assert!(parse_args(&args(&["notey", "+5"]), None).is_empty());
    }
}
