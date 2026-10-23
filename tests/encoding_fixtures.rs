//! Real files in every supported family: detection, decoding and byte-exact round trips.
use notey::application::file_io::{looks_binary, read_file, save_file};
use notey::core::encoding::{self, Confidence, Encoding};
use notey::core::line_ending::LineEnding;
use std::fs;
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/encodings").join(name)
}

struct Case {
    file: &'static str,
    encoding: &'static str,
    bom: bool,
    ending: LineEnding,
    certain: bool,
    /// A phrase that must survive decoding.
    contains: &'static str,
}

const CASES: &[Case] = &[
    Case { file: "utf8_plain.txt", encoding: "UTF-8", bom: false, ending: LineEnding::Lf, certain: true, contains: "second line" },
    Case { file: "utf8_bom_crlf.txt", encoding: "UTF-8", bom: true, ending: LineEnding::Crlf, certain: true, contains: "bom and crlf é" },
    Case { file: "utf16le_bom.txt", encoding: "UTF-16LE", bom: true, ending: LineEnding::Crlf, certain: true, contains: "héllo 𝄞" },
    Case { file: "utf16be_bom.txt", encoding: "UTF-16BE", bom: true, ending: LineEnding::Lf, certain: true, contains: "UTF-16 BE with BOM" },
    Case { file: "utf32le_bom.txt", encoding: "UTF-32LE", bom: true, ending: LineEnding::Lf, certain: true, contains: "UTF-32 LE with BOM 𝄞" },
    Case { file: "utf16le_nobom.txt", encoding: "UTF-16LE", bom: false, ending: LineEnding::Lf, certain: false, contains: "no byte order mark" },
    Case { file: "cr_only.txt", encoding: "UTF-8", bom: false, ending: LineEnding::Cr, certain: true, contains: "line two" },
    Case { file: "no_trailing_newline.txt", encoding: "UTF-8", bom: false, ending: LineEnding::Lf, certain: true, contains: "no newline" },
    Case { file: "empty.txt", encoding: "UTF-8", bom: false, ending: LineEnding::Lf, certain: true, contains: "" },
    Case { file: "cp1251.txt", encoding: "windows-1251", bom: false, ending: LineEnding::Lf, certain: false, contains: "Привет, мир!" },
    Case { file: "sjis.txt", encoding: "Shift_JIS", bom: false, ending: LineEnding::Lf, certain: false, contains: "日本語のテキスト" },
    Case { file: "gbk.txt", encoding: "GBK", bom: false, ending: LineEnding::Lf, certain: false, contains: "中文文本" },
    Case { file: "latin1_cafe.txt", encoding: "windows-1252", bom: false, ending: LineEnding::Lf, certain: false, contains: "café très crème brûlée" },
];

#[test]
fn every_fixture_is_detected_and_decoded() {
    for c in CASES {
        let bytes = fs::read(fixture(c.file)).unwrap();
        let d = encoding::decode(&bytes, None);
        assert_eq!(d.encoding.name(), c.encoding, "{}: encoding", c.file);
        assert_eq!(d.bom, c.bom, "{}: bom", c.file);
        assert_eq!(d.line_ending, c.ending, "{}: line ending", c.file);
        assert_eq!(d.confidence == Confidence::Certain, c.certain, "{}: confidence", c.file);
        assert!(d.text.contains(c.contains), "{}: text was {:?}", c.file, d.text);
        assert!(!d.had_errors, "{}: unexpected decode errors", c.file);
        assert!(!d.text.contains('\r'), "{}: buffer text must be \\n only", c.file);
    }
}

#[test]
fn every_fixture_round_trips_byte_for_byte() {
    for c in CASES {
        let bytes = fs::read(fixture(c.file)).unwrap();
        let d = encoding::decode(&bytes, None);
        let out = encoding::encode(&d.text, d.encoding, d.bom, d.line_ending);
        assert_eq!(out.bytes, bytes, "{}: re-encoding changed the bytes", c.file);
        assert!(out.unmappable.is_empty(), "{}", c.file);
    }
}

#[test]
fn opening_and_saving_through_the_file_layer_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    for c in CASES {
        let path = dir.path().join(c.file);
        fs::copy(fixture(c.file), &path).unwrap();
        let before = fs::read(&path).unwrap();
        let l = read_file(&path, None).unwrap();
        save_file(&path, &l.decoded.text, l.decoded.encoding, l.decoded.bom, l.decoded.line_ending).unwrap();
        assert_eq!(fs::read(&path).unwrap(), before, "{}", c.file);
    }
}

#[test]
fn mixed_endings_are_reported_and_normalised_to_the_majority() {
    let bytes = fs::read(fixture("mixed_endings.txt")).unwrap();
    let d = encoding::decode(&bytes, None);
    assert!(d.mixed_endings);
    assert_eq!(d.text, "unix\ncrlf\ncr\nend\n");
    assert_eq!(d.line_ending, LineEnding::Lf);
}

#[test]
fn the_binary_fixture_is_refused_as_text_but_utf16_is_not() {
    assert!(looks_binary(&fs::read(fixture("binary.bin")).unwrap()));
    assert!(read_file(&fixture("binary.bin"), None).unwrap().binary);
    for text in ["utf16le_bom.txt", "utf16be_bom.txt", "utf32le_bom.txt", "utf16le_nobom.txt"] {
        assert!(!looks_binary(&fs::read(fixture(text)).unwrap()), "{text} is text");
    }
}

#[test]
fn reopening_with_another_encoding_changes_the_text_not_the_file() {
    let bytes = fs::read(fixture("cp1251.txt")).unwrap();
    let wrong = encoding::decode(&bytes, Encoding::from_name("windows-1252"));
    let right = encoding::decode(&bytes, Encoding::from_name("windows-1251"));
    assert!(right.text.contains("Привет"));
    assert!(!wrong.text.contains("Привет"));
    // Forced decodes are always "certain": the user chose them.
    assert_eq!(wrong.confidence, Confidence::Certain);
}

#[test]
fn converting_between_encodings_reports_what_will_be_lost() {
    let d = encoding::decode(&fs::read(fixture("sjis.txt")).unwrap(), None);
    let to_latin1 = encoding::encode(&d.text, Encoding::from_name("windows-1252").unwrap(), false, LineEnding::Lf);
    assert!(!to_latin1.unmappable.is_empty());
    assert_eq!(to_latin1.unmappable[0].line, 1);
    let to_utf8 = encoding::encode(&d.text, Encoding::Utf8, false, LineEnding::Lf);
    assert!(to_utf8.unmappable.is_empty());
    assert_eq!(encoding::decode(&to_utf8.bytes, None).text, d.text);
}

#[test]
fn line_endings_convert_on_save() {
    let d = encoding::decode(&fs::read(fixture("utf8_plain.txt")).unwrap(), None);
    for (ending, sep) in [(LineEnding::Crlf, "\r\n"), (LineEnding::Cr, "\r"), (LineEnding::Lf, "\n")] {
        let out = encoding::encode(&d.text, Encoding::Utf8, false, ending).bytes;
        assert_eq!(String::from_utf8(out).unwrap(), format!("plain ascii{sep}second line{sep}"));
    }
}
