//! Detect, decode and encode text. Pure Rust: `encoding_rs` for the legacy and
//! UTF-8 paths; UTF-16/UTF-32 are done by hand because `encoding_rs` cannot
//! encode the former and does not know the latter.
use super::line_ending::{self, LineEnding};
use encoding_rs as enc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf16Le,
    Utf16Be,
    Utf32Le,
    Utf32Be,
    Legacy(&'static enc::Encoding),
}

/// Legacy encodings offered in menus, in display order. All of them can be
/// both decoded and encoded by `encoding_rs`.
const LEGACY_NAMES: &[&str] = &[
    "windows-1250", "windows-1251", "windows-1252", "windows-1253", "windows-1254",
    "windows-1255", "windows-1256", "windows-1257", "windows-1258", "windows-874",
    "ISO-8859-2", "ISO-8859-3", "ISO-8859-4", "ISO-8859-5", "ISO-8859-6", "ISO-8859-7",
    "ISO-8859-8", "ISO-8859-10", "ISO-8859-13", "ISO-8859-14", "ISO-8859-15", "ISO-8859-16",
    "KOI8-R", "KOI8-U", "IBM866", "macintosh", "GBK", "gb18030", "Big5", "EUC-JP",
    "Shift_JIS", "EUC-KR",
];

impl Encoding {
    pub const fn is_unicode(self) -> bool {
        !matches!(self, Encoding::Legacy(_))
    }

    /// Stable identifier used in the session and settings files.
    pub fn name(self) -> &'static str {
        match self {
            Encoding::Utf8 => "UTF-8",
            Encoding::Utf16Le => "UTF-16LE",
            Encoding::Utf16Be => "UTF-16BE",
            Encoding::Utf32Le => "UTF-32LE",
            Encoding::Utf32Be => "UTF-32BE",
            Encoding::Legacy(e) => e.name(),
        }
    }

    pub fn from_name(name: &str) -> Option<Encoding> {
        Some(match name {
            "UTF-8" => Encoding::Utf8,
            "UTF-16LE" => Encoding::Utf16Le,
            "UTF-16BE" => Encoding::Utf16Be,
            "UTF-32LE" => Encoding::Utf32Le,
            "UTF-32BE" => Encoding::Utf32Be,
            other => {
                let e = enc::Encoding::for_label(other.as_bytes())?;
                if !LEGACY_NAMES.contains(&e.name()) {
                    return None;
                }
                Encoding::Legacy(e)
            }
        })
    }

    pub fn all() -> Vec<Encoding> {
        let mut v = vec![
            Encoding::Utf8,
            Encoding::Utf16Le,
            Encoding::Utf16Be,
            Encoding::Utf32Le,
            Encoding::Utf32Be,
        ];
        v.extend(LEGACY_NAMES.iter().filter_map(|n| Encoding::from_name(n)));
        v
    }

    /// Human readable label for menus and the status bar.
    pub fn label(self) -> String {
        match self {
            Encoding::Utf8 => "UTF-8".into(),
            Encoding::Utf16Le => "UTF-16 LE".into(),
            Encoding::Utf16Be => "UTF-16 BE".into(),
            Encoding::Utf32Le => "UTF-32 LE".into(),
            Encoding::Utf32Be => "UTF-32 BE".into(),
            Encoding::Legacy(e) => e.name().to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    /// A byte order mark, valid UTF-8, or an encoding the caller chose.
    Certain,
    Guessed,
}

#[derive(Debug, Clone)]
pub struct Decoded {
    /// Always `\n`-normalised.
    pub text: String,
    pub encoding: Encoding,
    pub bom: bool,
    pub confidence: Confidence,
    /// Some bytes were invalid for the encoding and were replaced with U+FFFD.
    pub had_errors: bool,
    pub line_ending: LineEnding,
    pub mixed_endings: bool,
}

const SNIFF_LEN: usize = 64 * 1024;
const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

fn sniff_bom(bytes: &[u8]) -> Option<Encoding> {
    // UTF-32 LE starts with the UTF-16 LE mark, so it has to be tested first.
    if bytes.starts_with(&[0xFF, 0xFE, 0x00, 0x00]) {
        Some(Encoding::Utf32Le)
    } else if bytes.starts_with(&[0x00, 0x00, 0xFE, 0xFF]) {
        Some(Encoding::Utf32Be)
    } else if bytes.starts_with(UTF8_BOM) {
        Some(Encoding::Utf8)
    } else if bytes.starts_with(&[0xFF, 0xFE]) {
        Some(Encoding::Utf16Le)
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        Some(Encoding::Utf16Be)
    } else {
        None
    }
}

fn bom_len(encoding: Encoding, bytes: &[u8]) -> usize {
    let bom: &[u8] = match encoding {
        Encoding::Utf8 => UTF8_BOM,
        Encoding::Utf16Le => &[0xFF, 0xFE],
        Encoding::Utf16Be => &[0xFE, 0xFF],
        Encoding::Utf32Le => &[0xFF, 0xFE, 0x00, 0x00],
        Encoding::Utf32Be => &[0x00, 0x00, 0xFE, 0xFF],
        Encoding::Legacy(_) => return 0,
    };
    if bytes.starts_with(bom) {
        bom.len()
    } else {
        0
    }
}

/// UTF-16 without a mark: text in mostly-ASCII scripts has a NUL in every
/// other byte. Returns the byte order when at least 30% of the sampled bytes
/// are NULs and they sit consistently on one parity.
fn sniff_utf16_without_bom(bytes: &[u8]) -> Option<Encoding> {
    let sample = &bytes[..bytes.len().min(8192)];
    if sample.len() < 4 {
        return None;
    }
    let (mut even, mut odd) = (0usize, 0usize);
    for (i, b) in sample.iter().enumerate() {
        if *b == 0 {
            if i % 2 == 0 {
                even += 1;
            } else {
                odd += 1;
            }
        }
    }
    let threshold = sample.len() * 30 / 100;
    if odd >= threshold && even * 10 < odd {
        Some(Encoding::Utf16Le)
    } else if even >= threshold && odd * 10 < even {
        Some(Encoding::Utf16Be)
    } else {
        None
    }
}

fn decode_utf16(bytes: &[u8], big_endian: bool) -> (String, bool) {
    // The trailing odd byte, if any, is reported below; `as_chunks` leaves it out of the pairs.
    let units = bytes.as_chunks::<2>().0.iter().map(|c| {
        if big_endian {
            u16::from_be_bytes([c[0], c[1]])
        } else {
            u16::from_le_bytes([c[0], c[1]])
        }
    });
    let mut had_errors = !bytes.len().is_multiple_of(2);
    let mut out = String::with_capacity(bytes.len() / 2);
    for r in char::decode_utf16(units) {
        match r {
            Ok(c) => out.push(c),
            Err(_) => {
                had_errors = true;
                out.push('\u{FFFD}');
            }
        }
    }
    if !bytes.len().is_multiple_of(2) {
        out.push('\u{FFFD}');
    }
    (out, had_errors)
}

fn decode_utf32(bytes: &[u8], big_endian: bool) -> (String, bool) {
    let mut had_errors = !bytes.len().is_multiple_of(4);
    let mut out = String::with_capacity(bytes.len() / 4);
    for c in bytes.as_chunks::<4>().0 {
        let v = if big_endian {
            u32::from_be_bytes([c[0], c[1], c[2], c[3]])
        } else {
            u32::from_le_bytes([c[0], c[1], c[2], c[3]])
        };
        match char::from_u32(v) {
            Some(ch) => out.push(ch),
            None => {
                had_errors = true;
                out.push('\u{FFFD}');
            }
        }
    }
    if !bytes.len().is_multiple_of(4) {
        out.push('\u{FFFD}');
    }
    (out, had_errors)
}

fn decode_body(bytes: &[u8], encoding: Encoding) -> (String, bool) {
    match encoding {
        Encoding::Utf8 => {
            let (s, errors) = enc::UTF_8.decode_without_bom_handling(bytes);
            (s.into_owned(), errors)
        }
        Encoding::Utf16Le => decode_utf16(bytes, false),
        Encoding::Utf16Be => decode_utf16(bytes, true),
        Encoding::Utf32Le => decode_utf32(bytes, false),
        Encoding::Utf32Be => decode_utf32(bytes, true),
        Encoding::Legacy(e) => {
            let (s, errors) = e.decode_without_bom_handling(bytes);
            (s.into_owned(), errors)
        }
    }
}

fn finish(
    body: (String, bool),
    encoding: Encoding,
    bom: bool,
    confidence: Confidence,
) -> Decoded {
    let (raw, had_errors) = body;
    let detected = line_ending::detect(&raw);
    Decoded {
        text: line_ending::normalize(&raw),
        encoding,
        bom,
        confidence,
        had_errors,
        line_ending: detected.dominant,
        mixed_endings: detected.mixed,
    }
}

/// Decodes `bytes`. With `forced` the caller's encoding is used as is (a
/// matching byte order mark is still stripped and reported); otherwise the
/// encoding is detected: BOM, valid UTF-8, BOM-less UTF-16, then a statistical
/// guess that falls back to Windows-1252.
pub fn decode(bytes: &[u8], forced: Option<Encoding>) -> Decoded {
    if let Some(e) = forced {
        let skip = bom_len(e, bytes);
        return finish(decode_body(&bytes[skip..], e), e, skip > 0, Confidence::Certain);
    }
    if let Some(e) = sniff_bom(bytes) {
        let skip = bom_len(e, bytes);
        return finish(decode_body(&bytes[skip..], e), e, true, Confidence::Certain);
    }
    // NUL bytes are valid UTF-8, so BOM-less UTF-16 must be ruled in before the UTF-8 check.
    if let Some(e) = sniff_utf16_without_bom(bytes) {
        return finish(decode_body(bytes, e), e, false, Confidence::Guessed);
    }
    if std::str::from_utf8(bytes).is_ok() {
        return finish(decode_body(bytes, Encoding::Utf8), Encoding::Utf8, false, Confidence::Certain);
    }
    let mut detector = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
    let head = &bytes[..bytes.len().min(SNIFF_LEN)];
    detector.feed(head, head.len() == bytes.len());
    let mut guess = detector.guess(None, chardetng::Utf8Detection::Deny);
    if guess == enc::UTF_8 || !LEGACY_NAMES.contains(&guess.name()) {
        guess = enc::WINDOWS_1252;
    }
    let e = Encoding::Legacy(guess);
    finish(decode_body(bytes, e), e, false, Confidence::Guessed)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unmappable {
    /// 1-based.
    pub line: usize,
    /// 1-based, in characters.
    pub column: usize,
    pub ch: char,
}

#[derive(Debug, Clone)]
pub struct Encoded {
    pub bytes: Vec<u8>,
    /// Characters the encoding cannot represent. The encoder wrote HTML numeric
    /// references (`&#1234;`) in their place; empty for a lossless conversion.
    pub unmappable: Vec<Unmappable>,
}

/// Cap on reported characters so a binary-ish buffer cannot allocate without bound.
const MAX_REPORTED: usize = 10_000;

/// Which characters of `text` (`\n`-normalised) the encoding cannot represent.
pub fn check_convertible(text: &str, encoding: Encoding) -> Vec<Unmappable> {
    let Encoding::Legacy(e) = encoding else {
        return Vec::new();
    };
    // One pass over the whole text; only a lossy result is worth locating character by character.
    if !e.encode(text).2 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let (mut line, mut column) = (1usize, 1usize);
    let mut buf = [0u8; 4];
    for ch in text.chars() {
        if ch == '\n' {
            line += 1;
            column = 1;
            continue;
        }
        if !ch.is_ascii() {
            let (_, _, bad) = e.encode(ch.encode_utf8(&mut buf));
            if bad {
                out.push(Unmappable { line, column, ch });
                if out.len() >= MAX_REPORTED {
                    break;
                }
            }
        }
        column += 1;
    }
    out
}

/// Encodes `\n`-normalised `text`, applying the line ending and the byte order mark.
pub fn encode(text: &str, encoding: Encoding, bom: bool, ending: LineEnding) -> Encoded {
    let text = line_ending::apply(text, ending);
    let mut bytes = Vec::new();
    let mut unmappable = Vec::new();
    match encoding {
        Encoding::Utf8 => {
            if bom {
                bytes.extend_from_slice(UTF8_BOM);
            }
            bytes.extend_from_slice(text.as_bytes());
        }
        Encoding::Utf16Le | Encoding::Utf16Be => {
            let be = encoding == Encoding::Utf16Be;
            let put = |bytes: &mut Vec<u8>, u: u16| {
                bytes.extend_from_slice(&if be { u.to_be_bytes() } else { u.to_le_bytes() })
            };
            if bom {
                put(&mut bytes, 0xFEFF);
            }
            for u in text.encode_utf16() {
                put(&mut bytes, u);
            }
        }
        Encoding::Utf32Le | Encoding::Utf32Be => {
            let be = encoding == Encoding::Utf32Be;
            let put = |bytes: &mut Vec<u8>, u: u32| {
                bytes.extend_from_slice(&if be { u.to_be_bytes() } else { u.to_le_bytes() })
            };
            if bom {
                put(&mut bytes, 0xFEFF);
            }
            for c in text.chars() {
                put(&mut bytes, c as u32);
            }
        }
        Encoding::Legacy(e) => {
            // Positions are computed on the unconverted text so they match what the user sees.
            unmappable = check_convertible(&line_ending::normalize(&text), encoding);
            let (out, _, _) = e.encode(&text);
            bytes = out.into_owned();
        }
    }
    Encoded { bytes, unmappable }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cp1251(s: &str) -> Vec<u8> {
        enc::WINDOWS_1251.encode(s).0.into_owned()
    }

    #[test]
    fn empty_and_ascii_are_certain_utf8() {
        for b in [&b""[..], b"hello\n"] {
            let d = decode(b, None);
            assert_eq!(d.encoding, Encoding::Utf8);
            assert_eq!(d.confidence, Confidence::Certain);
            assert!(!d.bom);
        }
    }

    #[test]
    fn utf8_bom_is_stripped_and_remembered() {
        let d = decode(b"\xEF\xBB\xBFhi", None);
        assert_eq!((d.text.as_str(), d.bom, d.encoding), ("hi", true, Encoding::Utf8));
    }

    #[test]
    fn utf16_and_utf32_boms() {
        let cases = [
            (Encoding::Utf16Le, true),
            (Encoding::Utf16Be, true),
            (Encoding::Utf32Le, true),
            (Encoding::Utf32Be, true),
        ];
        for (e, bom) in cases {
            let bytes = encode("héllo 𝄞\n", e, bom, LineEnding::Lf).bytes;
            let d = decode(&bytes, None);
            assert_eq!(d.encoding, e, "{e:?}");
            assert!(d.bom);
            assert_eq!(d.text, "héllo 𝄞\n");
            assert!(!d.had_errors);
        }
    }

    #[test]
    fn utf32le_is_not_mistaken_for_utf16le() {
        let d = decode(&[0xFF, 0xFE, 0, 0, b'a', 0, 0, 0], None);
        assert_eq!(d.encoding, Encoding::Utf32Le);
        assert_eq!(d.text, "a");
    }

    #[test]
    fn utf16_without_bom_is_detected_as_a_guess() {
        let bytes = encode("hello world, this is text", Encoding::Utf16Le, false, LineEnding::Lf).bytes;
        let d = decode(&bytes, None);
        assert_eq!(d.encoding, Encoding::Utf16Le);
        assert_eq!(d.confidence, Confidence::Guessed);
        assert_eq!(d.text, "hello world, this is text");
        let bytes = encode("hello world, this is text", Encoding::Utf16Be, false, LineEnding::Lf).bytes;
        assert_eq!(decode(&bytes, None).encoding, Encoding::Utf16Be);
    }

    #[test]
    fn legacy_cyrillic_is_guessed() {
        let text = "Привет, мир! Это длинная строка текста для определения кодировки.";
        let d = decode(&cp1251(text), None);
        assert_eq!(d.confidence, Confidence::Guessed);
        assert!(matches!(d.encoding, Encoding::Legacy(_)));
        assert_eq!(d.text, text);
    }

    #[test]
    fn legacy_japanese_and_chinese_round_trip() {
        for (name, text) in [
            ("Shift_JIS", "これは日本語のテキストです。文字コードの判定を確認します。"),
            ("GBK", "这是一个用于检测字符编码的中文文本示例。"),
            ("KOI8-R", "Привет, мир! Это длинная строка текста."),
        ] {
            let e = Encoding::from_name(name).unwrap();
            let bytes = encode(text, e, false, LineEnding::Lf).bytes;
            assert_eq!(decode(&bytes, Some(e)).text, text, "{name}");
        }
    }

    #[test]
    fn undetectable_falls_back_to_a_legacy_encoding_never_utf8() {
        let d = decode(&[0x80, 0x81, 0xFF, 0xFE, 0x41], None);
        assert!(matches!(d.encoding, Encoding::Legacy(_)));
    }

    #[test]
    fn forced_encoding_wins_and_reports_errors() {
        let d = decode(b"caf\xE9", Some(Encoding::Utf8));
        assert!(d.had_errors);
        assert!(d.text.contains('\u{FFFD}'));
        assert_eq!(d.confidence, Confidence::Certain);
        let d = decode(b"caf\xE9", Some(Encoding::from_name("windows-1252").unwrap()));
        assert_eq!(d.text, "café");
        assert!(!d.had_errors);
    }

    #[test]
    fn invalid_utf16_and_utf32_are_flagged() {
        assert!(decode(&[0x00, 0xD8, 0x41, 0x00], Some(Encoding::Utf16Le)).had_errors);
        assert!(decode(&[0x41, 0x00, 0x00], Some(Encoding::Utf16Le)).had_errors);
        assert!(decode(&[0xFF, 0xFF, 0xFF, 0xFF], Some(Encoding::Utf32Le)).had_errors);
    }

    #[test]
    fn line_endings_are_detected_then_normalised() {
        let d = decode(b"a\r\nb\r\n", None);
        assert_eq!(d.text, "a\nb\n");
        assert_eq!(d.line_ending, LineEnding::Crlf);
        assert!(!d.mixed_endings);
        assert!(decode(b"a\r\nb\n", None).mixed_endings);
    }

    #[test]
    fn unchanged_files_round_trip_byte_for_byte() {
        let fixtures: Vec<(Vec<u8>, &str)> = vec![
            (b"plain\nascii\n".to_vec(), "lf"),
            (b"\xEF\xBB\xBFwith bom\r\nsecond".to_vec(), "bom crlf no trailing newline"),
            (cp1251("Привет\r\nмир\r\n"), "cp1251 crlf"),
            ("日本語\r".as_bytes().to_vec(), "cr only"),
            (encode("a\nb", Encoding::Utf16Le, true, LineEnding::Crlf).bytes, "utf16le"),
            (encode("a\nb\n", Encoding::Utf32Be, false, LineEnding::Lf).bytes, "utf32be no bom"),
        ];
        for (bytes, name) in fixtures {
            let d = decode(&bytes, None);
            let e = encode(&d.text, d.encoding, d.bom, d.line_ending);
            // UTF-32 without a BOM cannot be detected, so that fixture is expected to differ.
            if name != "utf32be no bom" {
                assert_eq!(e.bytes, bytes, "{name}");
            }
        }
    }

    #[test]
    fn unicode_text_round_trips_through_every_unicode_encoding() {
        let s = "Mixed: ASCII, é, ß, Ж, 日本, 𝄞, emoji 😀\nline two\n";
        for e in [Encoding::Utf8, Encoding::Utf16Le, Encoding::Utf16Be, Encoding::Utf32Le, Encoding::Utf32Be] {
            for bom in [true, false] {
                for ending in [LineEnding::Lf, LineEnding::Crlf, LineEnding::Cr] {
                    let bytes = encode(s, e, bom, ending).bytes;
                    let d = decode(&bytes, Some(e));
                    assert_eq!(d.text, s, "{e:?} bom={bom} {ending:?}");
                    assert_eq!(d.bom, bom);
                }
            }
        }
    }

    #[test]
    fn lossy_conversion_is_reported_with_positions() {
        let e = Encoding::from_name("windows-1252").unwrap();
        let out = encode("ok é\nbad Ж and 日\n", e, false, LineEnding::Lf);
        assert_eq!(
            out.unmappable,
            vec![
                Unmappable { line: 2, column: 5, ch: 'Ж' },
                Unmappable { line: 2, column: 11, ch: '日' },
            ]
        );
        assert!(check_convertible("plain é", e).is_empty());
        assert!(check_convertible("日本", Encoding::Utf16Le).is_empty());
    }

    #[test]
    fn names_round_trip_and_unsupported_are_rejected() {
        for e in Encoding::all() {
            assert_eq!(Encoding::from_name(e.name()), Some(e), "{}", e.name());
        }
        assert!(Encoding::from_name("ISO-2022-JP").is_none());
        assert!(Encoding::from_name("replacement").is_none());
        assert!(Encoding::from_name("x-user-defined").is_none());
        assert!(Encoding::from_name("nope").is_none());
        assert_eq!(Encoding::all().len(), 5 + LEGACY_NAMES.len());
    }
}
