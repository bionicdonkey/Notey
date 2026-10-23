//! Whole-line and selection transforms. Inputs are `\n`-separated text without
//! a trailing newline (the caller passes the selected lines); outputs follow the
//! same convention.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndentStyle {
    Tabs,
    Spaces(usize),
}

impl IndentStyle {
    pub fn unit(self) -> String {
        match self {
            IndentStyle::Tabs => "\t".into(),
            IndentStyle::Spaces(n) => " ".repeat(n.max(1)),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SortOptions {
    pub descending: bool,
    pub case_insensitive: bool,
    /// Compare runs of digits by value ("file2" before "file10").
    pub numeric: bool,
}

fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, _) => return std::cmp::Ordering::Less,
            (_, None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut s = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        s.push(c);
                        it.next();
                    }
                    s
                };
                let (da, db) = (take(&mut a), take(&mut b));
                let (ta, tb) = (da.trim_start_matches('0'), db.trim_start_matches('0'));
                let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if ord != std::cmp::Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                if x != y {
                    return x.cmp(&y);
                }
                a.next();
                b.next();
            }
        }
    }
}

pub fn sort_lines(text: &str, opts: SortOptions) -> String {
    let mut lines: Vec<&str> = text.split('\n').collect();
    let key = |s: &str| if opts.case_insensitive { s.to_lowercase() } else { s.to_string() };
    lines.sort_by(|a, b| {
        let (ka, kb) = (key(a), key(b));
        let ord = if opts.numeric { natural_cmp(&ka, &kb) } else { ka.cmp(&kb) };
        if opts.descending { ord.reverse() } else { ord }
    });
    lines.join("\n")
}

/// Keeps the first occurrence of each line.
pub fn dedupe_lines(text: &str) -> String {
    let mut seen = std::collections::HashSet::new();
    text.split('\n').filter(|l| seen.insert(*l)).collect::<Vec<_>>().join("\n")
}

pub fn remove_blank_lines(text: &str) -> String {
    text.split('\n').filter(|l| !l.trim().is_empty()).collect::<Vec<_>>().join("\n")
}

pub fn reverse_lines(text: &str) -> String {
    let mut lines: Vec<&str> = text.split('\n').collect();
    lines.reverse();
    lines.join("\n")
}

pub fn trim_trailing(text: &str) -> String {
    text.split('\n').map(str::trim_end).collect::<Vec<_>>().join("\n")
}

pub fn trim_leading(text: &str) -> String {
    text.split('\n').map(str::trim_start).collect::<Vec<_>>().join("\n")
}

/// Joins lines with a single space; the break and the next line's indentation
/// disappear, and blank lines are dropped.
pub fn join_lines(text: &str) -> String {
    let mut out = String::new();
    for line in text.split('\n') {
        let part = line.trim_start();
        if part.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.truncate(out.trim_end().len());
            out.push(' ');
        }
        out.push_str(part);
    }
    out
}

pub fn duplicate_lines(text: &str) -> String {
    format!("{text}\n{text}")
}

pub fn to_upper(text: &str) -> String {
    text.to_uppercase()
}

pub fn to_lower(text: &str) -> String {
    text.to_lowercase()
}

pub fn to_title(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut start = true;
    for c in text.chars() {
        if c.is_alphanumeric() || c == '\'' && !start {
            if start {
                out.extend(c.to_uppercase());
            } else {
                out.extend(c.to_lowercase());
            }
            start = false;
        } else {
            out.push(c);
            start = true;
        }
    }
    out
}

pub fn swap_case(text: &str) -> String {
    text.chars()
        .flat_map(|c| -> Vec<char> {
            if c.is_uppercase() {
                c.to_lowercase().collect()
            } else if c.is_lowercase() {
                c.to_uppercase().collect()
            } else {
                vec![c]
            }
        })
        .collect()
}

/// Prefixes every non-blank line with one indent unit.
pub fn indent(text: &str, style: IndentStyle) -> String {
    let unit = style.unit();
    text.split('\n')
        .map(|l| if l.trim().is_empty() { l.to_string() } else { format!("{unit}{l}") })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Removes one indent unit per line: a tab, or up to `width` spaces.
pub fn unindent(text: &str, width: usize) -> String {
    text.split('\n')
        .map(|l| {
            if let Some(rest) = l.strip_prefix('\t') {
                return rest;
            }
            let spaces = l.chars().take(width.max(1)).take_while(|c| *c == ' ').count();
            &l[spaces..]
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Moves lines `start..=end` (0-based) one step. Returns false at the edges.
pub fn move_lines(lines: &mut [String], start: usize, end: usize, up: bool) -> bool {
    if start > end || end >= lines.len() {
        return false;
    }
    if up {
        if start == 0 {
            return false;
        }
        lines[start - 1..=end].rotate_left(1);
    } else {
        if end + 1 >= lines.len() {
            return false;
        }
        lines[start..=end + 1].rotate_right(1);
    }
    true
}

/// Looks at up to 200 indented lines. Spaces win only when a common step
/// (2, 4 or 8) divides nearly all of the indents; otherwise tabs, if they lead.
pub fn detect_indent(text: &str) -> Option<IndentStyle> {
    let (mut tabs, mut spaces) = (0usize, 0usize);
    let mut widths = [0usize; 9];
    let mut seen = 0;
    let mut prev = 0usize;
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with('\t') {
            tabs += 1;
        } else if line.starts_with(' ') {
            let n = line.len() - line.trim_start_matches(' ').len();
            spaces += 1;
            // Continuation lines inside a block keep the previous indent; the
            // change between consecutive lines is what reveals the step.
            let diff = n.abs_diff(prev);
            if (1..=8).contains(&diff) {
                widths[diff] += 1;
            }
            prev = n;
        } else {
            prev = 0;
            continue;
        }
        seen += 1;
        if seen >= 200 {
            break;
        }
    }
    if tabs == 0 && spaces == 0 {
        return None;
    }
    if tabs > spaces {
        return Some(IndentStyle::Tabs);
    }
    let best = [2usize, 4, 8]
        .into_iter()
        .max_by_key(|w| widths[*w] + if *w == 4 { 1 } else { 0 })
        .unwrap_or(4);
    Some(IndentStyle::Spaces(best))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorting_variants() {
        let t = "b\nA\nc\na";
        assert_eq!(sort_lines(t, SortOptions::default()), "A\na\nb\nc");
        assert_eq!(sort_lines(t, SortOptions { case_insensitive: true, ..Default::default() }), "A\na\nb\nc");
        assert_eq!(sort_lines(t, SortOptions { descending: true, ..Default::default() }), "c\nb\na\nA");
        assert_eq!(
            sort_lines("f10\nf2\nf1", SortOptions { numeric: true, ..Default::default() }),
            "f1\nf2\nf10"
        );
        assert_eq!(sort_lines("f10\nf2\nf1", SortOptions::default()), "f1\nf10\nf2");
    }

    #[test]
    fn dedupe_blank_reverse() {
        assert_eq!(dedupe_lines("a\nb\na\nc\nb"), "a\nb\nc");
        assert_eq!(remove_blank_lines("a\n\n  \nb"), "a\nb");
        assert_eq!(reverse_lines("1\n2\n3"), "3\n2\n1");
    }

    #[test]
    fn trimming() {
        assert_eq!(trim_trailing("a  \n\tb\t \n"), "a\n\tb\n");
        assert_eq!(trim_leading("  a\n\tb"), "a\nb");
    }

    #[test]
    fn joining() {
        assert_eq!(join_lines("foo(\n    a,\n    b\n)"), "foo( a, b )");
        assert_eq!(join_lines("one  \n\n two"), "one two");
        assert_eq!(join_lines("solo"), "solo");
    }

    #[test]
    fn duplicate() {
        assert_eq!(duplicate_lines("a\nb"), "a\nb\na\nb");
    }

    #[test]
    fn case_conversion() {
        assert_eq!(to_upper("straße"), "STRASSE");
        assert_eq!(to_lower("ABC"), "abc");
        assert_eq!(to_title("hello wORLD it's-me"), "Hello World It's-Me");
        assert_eq!(swap_case("aBc1"), "AbC1");
    }

    #[test]
    fn indent_and_unindent() {
        assert_eq!(indent("a\n\nb", IndentStyle::Spaces(4)), "    a\n\n    b");
        assert_eq!(indent("a", IndentStyle::Tabs), "\ta");
        assert_eq!(unindent("    a\n  b\n\tc\nd", 4), "a\nb\nc\nd");
        assert_eq!(unindent("      a", 4), "  a");
    }

    #[test]
    fn indent_width_is_configurable() {
        assert_eq!(indent("x", IndentStyle::Spaces(2)), "  x");
        assert_eq!(indent("x", IndentStyle::Spaces(8)), "        x");
        assert_eq!(IndentStyle::Spaces(0).unit(), " ");
    }

    #[test]
    fn moving_lines() {
        let mut v: Vec<String> = ["a", "b", "c", "d"].iter().map(|s| s.to_string()).collect();
        assert!(move_lines(&mut v, 1, 2, true));
        assert_eq!(v, ["b", "c", "a", "d"]);
        assert!(move_lines(&mut v, 0, 1, false));
        assert_eq!(v, ["a", "b", "c", "d"]);
        assert!(!move_lines(&mut v, 0, 0, true));
        assert!(!move_lines(&mut v, 3, 3, false));
        assert!(!move_lines(&mut v, 2, 9, true));
    }

    #[test]
    fn indent_detection() {
        assert_eq!(detect_indent("a\n\tb\n\tc"), Some(IndentStyle::Tabs));
        assert_eq!(detect_indent("a\n  b\n    c\n  d"), Some(IndentStyle::Spaces(2)));
        assert_eq!(detect_indent("a\n    b\n        c\n    d"), Some(IndentStyle::Spaces(4)));
        assert_eq!(detect_indent("no indents\nat all"), None);
    }
}
