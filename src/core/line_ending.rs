use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineEnding {
    #[default]
    Lf,
    Crlf,
    Cr,
}

impl LineEnding {
    pub fn as_str(self) -> &'static str {
        match self {
            LineEnding::Lf => "\n",
            LineEnding::Crlf => "\r\n",
            LineEnding::Cr => "\r",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            LineEnding::Lf => "LF",
            LineEnding::Crlf => "CRLF",
            LineEnding::Cr => "CR",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Detected {
    pub dominant: LineEnding,
    pub mixed: bool,
}

/// Counts each style. Ties and text without any line break fall back to LF.
pub fn detect(text: &str) -> Detected {
    let (mut lf, mut crlf, mut cr) = (0usize, 0usize, 0usize);
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\r' if bytes.get(i + 1) == Some(&b'\n') => {
                crlf += 1;
                i += 1;
            }
            b'\r' => cr += 1,
            b'\n' => lf += 1,
            _ => {}
        }
        i += 1;
    }
    let styles = [lf, crlf, cr].iter().filter(|n| **n > 0).count();
    let dominant = if crlf > lf && crlf >= cr {
        LineEnding::Crlf
    } else if cr > lf && cr > crlf {
        LineEnding::Cr
    } else {
        LineEnding::Lf
    };
    Detected {
        dominant,
        mixed: styles > 1,
    }
}

/// Rewrites every line break as `\n`.
pub fn normalize(text: &str) -> String {
    if !text.contains('\r') {
        return text.to_string();
    }
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Expects `\n`-only text and rewrites the breaks in the target style.
pub fn apply(text: &str, ending: LineEnding) -> String {
    match ending {
        LineEnding::Lf => text.to_string(),
        other => text.replace('\n', other.as_str()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_each_style() {
        assert_eq!(detect("a\nb\n").dominant, LineEnding::Lf);
        assert_eq!(detect("a\r\nb\r\n").dominant, LineEnding::Crlf);
        assert_eq!(detect("a\rb\r").dominant, LineEnding::Cr);
        assert!(!detect("a\r\nb\r\n").mixed);
    }

    #[test]
    fn no_line_breaks_is_lf() {
        assert_eq!(detect(""), Detected { dominant: LineEnding::Lf, mixed: false });
        assert_eq!(detect("abc").dominant, LineEnding::Lf);
    }

    #[test]
    fn mixed_reports_the_majority() {
        let d = detect("a\r\nb\r\nc\nd");
        assert_eq!(d.dominant, LineEnding::Crlf);
        assert!(d.mixed);
    }

    #[test]
    fn crlf_is_not_counted_as_cr_plus_lf() {
        assert!(!detect("a\r\nb").mixed);
    }

    #[test]
    fn normalize_and_apply_round_trip() {
        let text = "one\r\ntwo\rthree\nfour";
        let norm = normalize(text);
        assert_eq!(norm, "one\ntwo\nthree\nfour");
        assert_eq!(apply(&norm, LineEnding::Crlf), "one\r\ntwo\r\nthree\r\nfour");
        assert_eq!(apply(&norm, LineEnding::Cr), "one\ntwo".replace('\n', "\r") + "\rthree\rfour");
        assert_eq!(apply(&norm, LineEnding::Lf), norm);
    }

    #[test]
    fn trailing_newline_is_preserved() {
        assert_eq!(apply("a\n", LineEnding::Crlf), "a\r\n");
        assert_eq!(normalize("a\r\n"), "a\n");
    }
}
