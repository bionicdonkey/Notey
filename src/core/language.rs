//! Language guessing for files GtkSourceView's own detection does not recognise.
//! Returns GtkSourceView language ids.

fn by_file_name(name: &str) -> Option<&'static str> {
    Some(match name {
        "Makefile" | "makefile" | "GNUmakefile" => "makefile",
        "Dockerfile" | "Containerfile" => "dockerfile",
        "CMakeLists.txt" => "cmake",
        ".bashrc" | ".bash_profile" | ".bash_aliases" | ".profile" | ".zshrc" | ".zprofile"
        | "PKGBUILD" => "sh",
        ".gitignore" | ".dockerignore" | ".gitattributes" => "gitignore",
        "Gemfile" | "Rakefile" => "ruby",
        _ => return None,
    })
}

/// Maps an interpreter, vim filetype or emacs mode name to a language id.
fn by_name(name: &str) -> Option<&'static str> {
    let name = name.to_ascii_lowercase();
    let base = name.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
    Some(match base {
        "python" if name.starts_with("python2") => "python",
        "python" => "python3",
        "sh" | "bash" | "zsh" | "dash" | "ksh" | "shell-script" => "sh",
        "perl" => "perl",
        "ruby" => "ruby",
        "node" | "nodejs" | "javascript" | "js" => "js",
        "typescript" | "ts" => "typescript",
        "php" => "php",
        "lua" => "lua",
        "c" => "c",
        "cpp" | "c++" => "cpp",
        "rust" => "rust",
        "go" => "go",
        "java" => "java",
        "yaml" => "yaml",
        "json" => "json",
        "markdown" | "md" => "markdown",
        "html" => "html",
        "xml" => "xml",
        "css" => "css",
        "sql" => "sql",
        "make" | "makefile" => "makefile",
        "dockerfile" => "dockerfile",
        "toml" => "toml",
        _ => return None,
    })
}

fn by_shebang(first_line: &str) -> Option<&'static str> {
    let rest = first_line.strip_prefix("#!")?.trim();
    let mut parts = rest.split_whitespace();
    let mut prog = parts.next()?.rsplit('/').next()?;
    if prog == "env" {
        // `env -S python3 -u` and `env VAR=1 python3`: take the first plain word.
        prog = parts.find(|p| !p.starts_with('-') && !p.contains('='))?;
    }
    by_name(prog)
}

fn by_modeline(text: &str) -> Option<&'static str> {
    let lower = text.to_ascii_lowercase();
    for key in ["ft=", "filetype=", "syntax="] {
        if let Some(i) = lower.find(key) {
            if lower[..i].contains("vim:") || lower[..i].contains("vi:") || lower[..i].contains("ex:") {
                let v: String = lower[i + key.len()..]
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '+' || *c == '-')
                    .collect();
                if let Some(id) = by_name(&v) {
                    return Some(id);
                }
            }
        }
    }
    if let Some(i) = lower.find("-*-") {
        let inner = &lower[i + 3..];
        if let Some(end) = inner.find("-*-") {
            let body = &inner[..end];
            if let Some(m) = body.find("mode:") {
                let v: String = body[m + 5..]
                    .trim_start()
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '+' || *c == '-')
                    .collect();
                return by_name(&v);
            }
            // `-*- python -*-`
            return by_name(body.trim());
        }
    }
    None
}

/// `head` and `tail` are the first and last few lines of the file.
pub fn fallback_language(file_name: Option<&str>, head: &str, tail: &str) -> Option<&'static str> {
    if let Some(id) = file_name.and_then(by_file_name) {
        return Some(id);
    }
    if let Some(id) = head.lines().next().and_then(by_shebang) {
        return Some(id);
    }
    head.lines()
        .take(5)
        .chain(tail.lines().rev().take(5))
        .find_map(by_modeline)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names() {
        assert_eq!(fallback_language(Some("Makefile"), "", ""), Some("makefile"));
        assert_eq!(fallback_language(Some("Dockerfile"), "", ""), Some("dockerfile"));
        assert_eq!(fallback_language(Some(".bashrc"), "", ""), Some("sh"));
        assert_eq!(fallback_language(Some("notes.txt"), "", ""), None);
    }

    #[test]
    fn shebangs() {
        let f = |s| fallback_language(None, s, "");
        assert_eq!(f("#!/usr/bin/env python3"), Some("python3"));
        assert_eq!(f("#!/usr/bin/python3.11"), Some("python3"));
        assert_eq!(f("#!/usr/bin/python2"), Some("python"));
        assert_eq!(f("#!/bin/bash -e"), Some("sh"));
        assert_eq!(f("#!/usr/bin/env -S node --flag"), Some("js"));
        assert_eq!(f("#!/usr/bin/env FOO=1 ruby"), Some("ruby"));
        assert_eq!(f("#!/usr/bin/perl\nprint 1"), Some("perl"));
        assert_eq!(f("# not a shebang"), None);
        assert_eq!(f("#!/opt/weird"), None);
    }

    #[test]
    fn modelines() {
        assert_eq!(fallback_language(None, "x\n# vim: set ft=python :", ""), Some("python3"));
        assert_eq!(fallback_language(None, "", "a\n// vim: filetype=rust"), Some("rust"));
        assert_eq!(fallback_language(None, "# -*- mode: ruby -*-", ""), Some("ruby"));
        assert_eq!(fallback_language(None, "# -*- python -*-", ""), Some("python3"));
        assert_eq!(fallback_language(None, "ft=python without vim marker", ""), None);
    }

    #[test]
    fn file_name_beats_shebang() {
        assert_eq!(fallback_language(Some("Makefile"), "#!/bin/bash", ""), Some("makefile"));
    }
}
