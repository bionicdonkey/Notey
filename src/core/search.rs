//! Search query model. The editor runs the search through GtkSourceView (PCRE2
//! via GLib), so everything here only prepares patterns and replacements.

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Query {
    pub pattern: String,
    pub regex: bool,
    pub case_sensitive: bool,
    pub whole_word: bool,
}

/// Escapes every ASCII punctuation character; PCRE2 treats `\` + non-alphanumeric as literal.
pub fn escape_literal(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        if c.is_ascii() && !c.is_ascii_alphanumeric() && c != '_' && c != '\n' && c != ' ' {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The regular expression source equivalent to the query, used where a plain
/// regex is needed (the all-tabs scan). Whole-word wraps in `\b`.
pub fn regex_source(q: &Query) -> String {
    let body = if q.regex { q.pattern.clone() } else { escape_literal(&q.pattern) };
    if q.whole_word && q.regex {
        format!(r"\b(?:{body})\b")
    } else if q.whole_word {
        format!(r"\b{body}\b")
    } else {
        body
    }
}

/// Converts `$1`, `${1}`, `$0` to the `\1` form GLib expects and `$$` to `$`.
/// Backslash escapes (`\n`, `\t`, `\\`, `\1`) pass through untouched.
pub fn translate_replacement(rep: &str) -> String {
    let chars: Vec<char> = rep.chars().collect();
    let mut out = String::with_capacity(rep.len());
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' => {
                out.push('\\');
                if let Some(next) = chars.get(i + 1) {
                    out.push(*next);
                }
                i += 2;
            }
            '$' => match chars.get(i + 1) {
                Some('$') => {
                    out.push('$');
                    i += 2;
                }
                Some(d) if d.is_ascii_digit() => {
                    out.push('\\');
                    out.push(*d);
                    i += 2;
                }
                Some('{') => {
                    let close = chars[i + 2..].iter().position(|c| *c == '}');
                    match close {
                        Some(n) if n > 0 && chars[i + 2..i + 2 + n].iter().all(char::is_ascii_digit) => {
                            let digits: String = chars[i + 2..i + 2 + n].iter().collect();
                            out.push_str(&format!("\\g<{digits}>"));
                            i += n + 3;
                        }
                        _ => {
                            out.push('$');
                            i += 1;
                        }
                    }
                }
                _ => {
                    out.push('$');
                    i += 1;
                }
            },
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_escaping() {
        assert_eq!(escape_literal("a.b*c(d)"), r"a\.b\*c\(d\)");
        assert_eq!(escape_literal("snake_case 1"), "snake_case 1");
        assert_eq!(escape_literal("a\nb"), "a\nb");
        assert_eq!(escape_literal("é-ü"), "é\\-ü");
    }

    #[test]
    fn regex_source_wraps_whole_words() {
        let q = |p: &str, regex, whole_word| Query { pattern: p.into(), regex, case_sensitive: false, whole_word };
        assert_eq!(regex_source(&q("a.b", false, false)), r"a\.b");
        assert_eq!(regex_source(&q("a.b", true, false)), "a.b");
        assert_eq!(regex_source(&q("cat", false, true)), r"\bcat\b");
        assert_eq!(regex_source(&q("a|b", true, true)), r"\b(?:a|b)\b");
    }

    #[test]
    fn dollar_references_are_translated() {
        assert_eq!(translate_replacement("$1-$2"), r"\1-\2");
        assert_eq!(translate_replacement("${1}0"), r"\g<1>0");
        assert_eq!(translate_replacement("$0!"), r"\0!");
        assert_eq!(translate_replacement("cost: $$5"), "cost: $5");
        assert_eq!(translate_replacement("$x and $"), "$x and $");
        assert_eq!(translate_replacement("${name}"), "${name}");
    }

    #[test]
    fn backslash_escapes_pass_through() {
        assert_eq!(translate_replacement(r"\1\n\t\\$1"), r"\1\n\t\\\1");
        assert_eq!(translate_replacement("end\\"), "end\\");
    }
}
