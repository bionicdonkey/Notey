//! Toggle line and block comments. Pure string transforms; the editor passes the
//! text of the touched lines (or the selection) and replaces it with the result.

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CommentSyntax {
    pub line: Option<String>,
    pub block: Option<(String, String)>,
}

impl CommentSyntax {
    pub fn is_empty(&self) -> bool {
        self.line.is_none() && self.block.is_none()
    }
}

fn line(m: &str) -> CommentSyntax {
    CommentSyntax { line: Some(m.into()), block: None }
}

fn block(a: &str, b: &str) -> CommentSyntax {
    CommentSyntax { line: None, block: Some((a.into(), b.into())) }
}

/// Fallbacks for GtkSourceView language ids whose definition carries no comment
/// metadata. `Some(empty)` means "this language has no comments" (JSON, plain text).
pub fn fallback_syntax(language_id: &str) -> Option<CommentSyntax> {
    Some(match language_id {
        "rust" | "c" | "cpp" | "chdr" | "c-sharp" | "go" | "java" | "kotlin" | "swift"
        | "js" | "typescript" | "scala" | "dart" | "groovy" | "objc" => line("//"),
        "python" | "python3" | "ruby" | "perl" | "sh" | "yaml" | "toml" | "dockerfile"
        | "cmake" | "makefile" | "desktop" | "r" | "powershell" | "nix" | "gitignore" => line("#"),
        "php" => line("//"),
        "lua" | "sql" | "haskell" => line("--"),
        "ini" => line(";"),
        "latex" | "bibtex" => line("%"),
        "html" | "xml" | "markdown" | "xslt" => block("<!--", "-->"),
        "css" | "scss" | "less" => block("/*", "*/"),
        "json" | "plain" | "text" | "diff" | "log" => CommentSyntax::default(),
        _ => return None,
    })
}

/// Prefers the language's own metadata, then the fallback table. Never panics for unknown ids.
pub fn resolve(
    language_id: Option<&str>,
    meta_line: Option<&str>,
    meta_block: Option<(&str, &str)>,
) -> CommentSyntax {
    let from_meta = CommentSyntax {
        line: meta_line.filter(|s| !s.is_empty()).map(Into::into),
        block: meta_block
            .filter(|(a, b)| !a.is_empty() && !b.is_empty())
            .map(|(a, b)| (a.into(), b.into())),
    };
    if !from_meta.is_empty() {
        return from_meta;
    }
    language_id.and_then(fallback_syntax).unwrap_or_default()
}

#[derive(Clone, Copy)]
enum Marker<'a> {
    Line(&'a str),
    Wrap(&'a str, &'a str),
}

fn indent_len(l: &str) -> usize {
    l.len() - l.trim_start().len()
}

fn is_commented(l: &str, m: Marker) -> bool {
    let body = l.trim_start();
    match m {
        Marker::Line(p) => match body.strip_prefix(p) {
            // `///` and `//!` are doc comments, not something this command put there.
            Some(rest) if p == "//" => !rest.starts_with('/') && !rest.starts_with('!'),
            Some(_) => true,
            None => false,
        },
        Marker::Wrap(a, b) => {
            let body = body.trim_end();
            body.len() >= a.len() + b.len() && body.starts_with(a) && body.ends_with(b)
        }
    }
}

fn uncomment(l: &str, m: Marker) -> String {
    let ind = indent_len(l);
    let (lead, body) = l.split_at(ind);
    match m {
        Marker::Line(p) => {
            let rest = &body[p.len()..];
            format!("{lead}{}", rest.strip_prefix(' ').unwrap_or(rest))
        }
        Marker::Wrap(a, b) => {
            let trimmed = body.trim_end();
            let tail = &body[trimmed.len()..];
            let inner = &trimmed[a.len()..trimmed.len() - b.len()];
            let inner = inner.strip_prefix(' ').unwrap_or(inner);
            let inner = inner.strip_suffix(' ').unwrap_or(inner);
            format!("{lead}{inner}{tail}")
        }
    }
}

fn toggle_lines(text: &str, m: Marker) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let live: Vec<&&str> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
    if live.is_empty() {
        return text.to_string();
    }
    if live.iter().all(|l| is_commented(l, m)) {
        return lines
            .iter()
            .map(|l| if l.trim().is_empty() { l.to_string() } else { uncomment(l, m) })
            .collect::<Vec<_>>()
            .join("\n");
    }
    // Comment at the shallowest indentation so the block stays aligned.
    let col = live
        .iter()
        .map(|l| l[..indent_len(l)].chars().count())
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .map(|l| {
            if l.trim().is_empty() {
                return l.to_string();
            }
            let at = l.char_indices().nth(col).map_or(l.len(), |(i, _)| i);
            let (lead, rest) = l.split_at(at);
            match m {
                Marker::Line(p) => format!("{lead}{p} {rest}"),
                Marker::Wrap(a, b) => {
                    let body = rest.trim_end();
                    let tail = &rest[body.len()..];
                    format!("{lead}{a} {body} {b}{tail}")
                }
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether every non-blank line is already commented: `Some(true)` all, `Some(false)` not all,
/// `None` when there is nothing to comment or the language has no comment syntax.
pub fn line_comment_state(text: &str, syntax: &CommentSyntax) -> Option<bool> {
    let m = match (&syntax.line, &syntax.block) {
        (Some(p), _) => Marker::Line(p),
        (None, Some((a, b))) => Marker::Wrap(a, b),
        _ => return None,
    };
    let mut live = text.split('\n').filter(|l| !l.trim().is_empty()).peekable();
    live.peek()?;
    Some(live.all(|l| is_commented(l, m)))
}

/// The "toggle comment" command. Uses the line marker when the language has one,
/// otherwise wraps each line in the block markers. `None`: no comment syntax.
pub fn toggle_line_comment(text: &str, syntax: &CommentSyntax) -> Option<String> {
    if let Some(p) = &syntax.line {
        Some(toggle_lines(text, Marker::Line(p)))
    } else {
        syntax.block.as_ref().map(|(a, b)| toggle_lines(text, Marker::Wrap(a, b)))
    }
}

/// The "block comment" command: wraps the whole text once, or unwraps it.
/// `None`: the language has no block comment.
pub fn toggle_block_comment(text: &str, syntax: &CommentSyntax) -> Option<String> {
    let (a, b) = syntax.block.as_ref()?;
    let lead_len = indent_len(text);
    let body = text[lead_len..].trim_end();
    let (lead, tail) = (&text[..lead_len], &text[lead_len + body.len()..]);
    if body.is_empty() {
        return Some(text.to_string());
    }
    if is_commented(body, Marker::Wrap(a, b)) {
        return Some(uncomment(text, Marker::Wrap(a, b)));
    }
    Some(format!("{lead}{a} {body} {b}{tail}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sl(m: &str) -> CommentSyntax { line(m) }

    #[test]
    fn comments_at_minimum_indentation_and_skips_blank_lines() {
        let t = "    a\n\n      b\n    c";
        assert_eq!(
            toggle_line_comment(t, &sl("//")).unwrap(),
            "    // a\n\n    //   b\n    // c"
        );
    }

    #[test]
    fn uncomments_when_everything_is_commented() {
        assert_eq!(
            toggle_line_comment("    // a\n\n    //   b\n    //c", &sl("//")).unwrap(),
            "    a\n\n      b\n    c"
        );
    }

    #[test]
    fn mixed_state_comments_everything() {
        assert_eq!(toggle_line_comment("# a\nb", &sl("#")).unwrap(), "# # a\n# b");
    }

    #[test]
    fn round_trip_restores_the_original() {
        let t = "fn x() {\n    let y = 1;\n\n\treturn;\n}";
        for m in ["//", "#", "--"] {
            let c = toggle_line_comment(t, &sl(m)).unwrap();
            assert_ne!(c, t);
            assert_eq!(toggle_line_comment(&c, &sl(m)).unwrap(), t, "{m}");
        }
    }

    #[test]
    fn blank_only_selection_is_untouched() {
        assert_eq!(toggle_line_comment("\n  \n", &sl("#")).unwrap(), "\n  \n");
    }

    #[test]
    fn doc_comments_are_not_uncommented_as_plain_ones() {
        let t = "/// doc\n//! inner";
        // Neither line counts as commented by this command, so it comments them.
        assert_eq!(toggle_line_comment(t, &sl("//")).unwrap(), "// /// doc\n// //! inner");
        // But a plain `//` line mixed with a doc comment uncommenting only works when all are plain.
        assert_eq!(toggle_line_comment("// a\n/// b", &sl("//")).unwrap(), "// // a\n// /// b");
    }

    #[test]
    fn tab_indentation_is_preserved() {
        assert_eq!(toggle_line_comment("\ta\n\t\tb", &sl("#")).unwrap(), "\t# a\n\t# \tb");
    }

    #[test]
    fn block_only_languages_wrap_each_line() {
        let html = block("<!--", "-->");
        let c = toggle_line_comment("<p>a</p>\n  <p>b</p>", &html).unwrap();
        assert_eq!(c, "<!-- <p>a</p> -->\n<!--   <p>b</p> -->");
        assert_eq!(toggle_line_comment(&c, &html).unwrap(), "<p>a</p>\n  <p>b</p>");
    }

    #[test]
    fn block_toggle_wraps_and_unwraps_the_whole_text() {
        let css = block("/*", "*/");
        let c = toggle_block_comment("  a: 1;\nb: 2;", &css).unwrap();
        assert_eq!(c, "  /* a: 1;\nb: 2; */");
        assert_eq!(toggle_block_comment(&c, &css).unwrap(), "  a: 1;\nb: 2;");
        assert_eq!(toggle_block_comment("x", &sl("#")), None);
    }

    #[test]
    fn comment_state() {
        assert_eq!(line_comment_state("# a\n\n# b", &sl("#")), Some(true));
        assert_eq!(line_comment_state("# a\nb", &sl("#")), Some(false));
        assert_eq!(line_comment_state("\n ", &sl("#")), None);
        assert_eq!(line_comment_state("x", &CommentSyntax::default()), None);
        assert_eq!(line_comment_state("<!-- a -->", &block("<!--", "-->")), Some(true));
    }

    #[test]
    fn languages_without_comments_report_none() {
        assert_eq!(toggle_line_comment("{}", &CommentSyntax::default()), None);
        assert!(resolve(Some("json"), None, None).is_empty());
    }

    #[test]
    fn resolve_prefers_metadata_then_table() {
        assert_eq!(resolve(Some("yaml"), Some(";"), None), sl(";"));
        assert_eq!(resolve(Some("yaml"), None, None), sl("#"));
        assert_eq!(resolve(Some("html"), None, None), block("<!--", "-->"));
        assert!(resolve(Some("unknown-lang"), None, None).is_empty());
        assert!(resolve(None, Some(""), Some(("", ""))).is_empty());
    }
}
