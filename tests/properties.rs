//! Property tests over the pure core: whatever the input, the invariants hold.
use notey::core::comment::{toggle_line_comment, CommentSyntax};
use notey::core::encoding::{self, Encoding};
use notey::core::line_ending::{self, LineEnding};
use notey::core::search::{escape_literal, translate_replacement};
use notey::core::session::{Cursor, Session, TabRecord};
use notey::core::text_ops::{self, IndentStyle, SortOptions};
use notey::ui::menus::move_entry;
use proptest::prelude::*;

fn any_ending() -> impl Strategy<Value = LineEnding> {
    prop_oneof![Just(LineEnding::Lf), Just(LineEnding::Crlf), Just(LineEnding::Cr)]
}

fn unicode_encoding() -> impl Strategy<Value = Encoding> {
    prop_oneof![
        Just(Encoding::Utf8),
        Just(Encoding::Utf16Le),
        Just(Encoding::Utf16Be),
        Just(Encoding::Utf32Le),
        Just(Encoding::Utf32Be),
    ]
}

/// Text as the buffer holds it: `\n` only, no carriage returns, any Unicode.
fn buffer_text() -> impl Strategy<Value = String> {
    // A leading U+FEFF is indistinguishable from a byte order mark, so it is not generated.
    "[^\r]{0,200}".prop_filter("leading U+FEFF reads as a BOM", |s| !s.starts_with('\u{feff}'))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn unicode_encodings_round_trip_any_text(s in buffer_text(), e in unicode_encoding(), bom: bool, le in any_ending()) {
        let bytes = encoding::encode(&s, e, bom, le).bytes;
        let d = encoding::decode(&bytes, Some(e));
        prop_assert_eq!(&d.text, &s);
        prop_assert_eq!(d.bom, bom);
        prop_assert!(!d.had_errors);
    }

    #[test]
    fn detection_never_loses_text_for_utf8(s in buffer_text()) {
        let bytes = s.as_bytes();
        let d = encoding::decode(bytes, None);
        // Valid UTF-8 without NULs (the only ambiguity is BOM-less UTF-16) is recognised exactly.
        if !bytes.contains(&0) && !s.starts_with('\u{feff}') {
            prop_assert_eq!(d.encoding, Encoding::Utf8);
            prop_assert_eq!(d.text, s);
        }
    }

    #[test]
    fn decoding_arbitrary_bytes_never_panics_and_is_stable(bytes in proptest::collection::vec(any::<u8>(), 0..400)) {
        let d = encoding::decode(&bytes, None);
        prop_assert!(!d.text.contains('\r'));
        // Some legacy decoders (Big5 extensions, say) yield characters their encoder cannot write
        // back; those are what the lossy-save warning exists for, so only lossless cases are compared.
        let out = encoding::encode(&d.text, d.encoding, d.bom, d.line_ending);
        if out.unmappable.is_empty() {
            let again = encoding::decode(&out.bytes, Some(d.encoding));
            prop_assert_eq!(again.text, d.text);
        }
    }

    #[test]
    fn legacy_decode_then_encode_is_stable(bytes in proptest::collection::vec(any::<u8>(), 0..200), idx in 0usize..20) {
        let legacy: Vec<Encoding> = Encoding::all().into_iter().filter(|e| !e.is_unicode()).collect();
        let e = legacy[idx % legacy.len()];
        let d = encoding::decode(&bytes, Some(e));
        let out = encoding::encode(&d.text, e, false, d.line_ending);
        if !d.had_errors && out.unmappable.is_empty() {
            let back = encoding::decode(&out.bytes, Some(e));
            prop_assert_eq!(back.text, d.text);
        }
    }

    #[test]
    fn check_convertible_agrees_with_encode(s in buffer_text(), idx in 0usize..20) {
        let legacy: Vec<Encoding> = Encoding::all().into_iter().filter(|e| !e.is_unicode()).collect();
        let e = legacy[idx % legacy.len()];
        let checked = encoding::check_convertible(&s, e);
        let encoded = encoding::encode(&s, e, false, LineEnding::Lf);
        prop_assert_eq!(checked.len(), encoded.unmappable.len());
        for u in &checked {
            prop_assert!(!u.ch.is_ascii());
            prop_assert!(u.line >= 1 && u.column >= 1);
        }
    }

    #[test]
    fn line_ending_conversion_round_trips(s in "[a-z \n]{0,100}", le in any_ending()) {
        let applied = line_ending::apply(&s, le);
        prop_assert_eq!(line_ending::normalize(&applied), s.clone());
        let d = line_ending::detect(&applied);
        if s.contains('\n') {
            prop_assert_eq!(d.dominant, le);
            prop_assert!(!d.mixed);
        }
    }

    #[test]
    fn normalize_is_idempotent_and_removes_every_carriage_return(s in "[a-z\r\n]{0,100}") {
        let once = line_ending::normalize(&s);
        prop_assert!(!once.contains('\r'));
        prop_assert_eq!(line_ending::normalize(&once), once);
    }

    #[test]
    fn comment_toggle_twice_restores_the_text(
        lines in proptest::collection::vec("[ \t]{0,6}[a-z0-9][a-z0-9 ]{0,10}|[ \t]{0,3}", 1..8),
        marker in prop_oneof![Just("//"), Just("#"), Just("--"), Just(";")],
    ) {
        let text = lines.join("\n");
        let syntax = CommentSyntax { line: Some(marker.to_string()), block: None };
        let once = toggle_line_comment(&text, &syntax).unwrap();
        let twice = toggle_line_comment(&once, &syntax).unwrap();
        prop_assert_eq!(twice, text);
    }

    #[test]
    fn comment_toggle_keeps_the_line_count(
        lines in proptest::collection::vec("[ \t]{0,4}[a-z]{0,8}", 1..10),
    ) {
        let text = lines.join("\n");
        let syntax = CommentSyntax { line: Some("//".into()), block: Some(("/*".into(), "*/".into())) };
        let once = toggle_line_comment(&text, &syntax).unwrap();
        prop_assert_eq!(once.split('\n').count(), text.split('\n').count());
    }

    #[test]
    fn sorting_is_idempotent_and_a_permutation(s in "[a-zA-Z0-9\n]{0,80}", desc: bool, ci: bool, numeric: bool) {
        let o = SortOptions { descending: desc, case_insensitive: ci, numeric };
        let once = text_ops::sort_lines(&s, o);
        prop_assert_eq!(text_ops::sort_lines(&once, o), once.clone());
        let mut a: Vec<&str> = s.split('\n').collect();
        let mut b: Vec<&str> = once.split('\n').collect();
        a.sort();
        b.sort();
        prop_assert_eq!(a, b);
    }

    #[test]
    fn dedupe_and_trim_are_idempotent(s in "[a-c \n]{0,60}") {
        let d = text_ops::dedupe_lines(&s);
        prop_assert_eq!(text_ops::dedupe_lines(&d), d.clone());
        let lines: Vec<&str> = d.split('\n').collect();
        let unique: std::collections::HashSet<&&str> = lines.iter().collect();
        prop_assert_eq!(lines.len(), unique.len());
        let t = text_ops::trim_trailing(&s);
        prop_assert_eq!(text_ops::trim_trailing(&t), t);
    }

    #[test]
    fn indent_then_unindent_restores_text(
        lines in proptest::collection::vec("[ ]{0,5}[a-z][a-z ]{0,8}", 1..6),
        width in 1usize..9,
    ) {
        let text = lines.join("\n");
        let style = IndentStyle::Spaces(width);
        let restored = text_ops::unindent(&text_ops::indent(&text, style), width);
        prop_assert_eq!(restored, text.clone());
        let tabs = text_ops::unindent(&text_ops::indent(&text, IndentStyle::Tabs), width);
        prop_assert_eq!(tabs, text);
    }

    #[test]
    fn join_lines_leaves_a_single_line(s in "[a-z \n]{0,60}") {
        prop_assert!(!text_ops::join_lines(&s).contains('\n'));
    }

    #[test]
    fn case_conversions_never_panic(s in "\\PC{0,60}") {
        let _ = (text_ops::to_upper(&s), text_ops::to_lower(&s), text_ops::to_title(&s), text_ops::swap_case(&s));
    }

    #[test]
    fn replacement_translation_never_panics_and_keeps_plain_text(s in "\\PC{0,60}") {
        let _ = translate_replacement(&s);
        let plain: String = s.chars().filter(|c| *c != '$' && *c != '\\').collect();
        prop_assert_eq!(translate_replacement(&plain), plain);
    }

    #[test]
    fn escaped_literals_have_no_bare_metacharacters(s in "\\PC{0,40}") {
        let esc = escape_literal(&s);
        let chars: Vec<char> = esc.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '\\' {
                i += 2; // an escape consumes the next character
                continue;
            }
            prop_assert!(!"()[]{}.*+?^$|#&-~".contains(chars[i]), "bare {:?} in {:?}", chars[i], esc);
            i += 1;
        }
    }

    #[test]
    fn dragging_a_row_is_always_a_permutation(
        items in proptest::collection::vec(prop_oneof![Just("undo"), Just("|"), Just("cut"), Just("copy"), Just("bogus"), Just("find")], 0..12),
        from in 0usize..14,
        to in 0usize..14,
    ) {
        let mut list: Vec<String> = items.iter().map(|s| s.to_string()).collect();
        let mut before = list.clone();
        move_entry(&mut list, from, to);
        before.sort();
        list.sort();
        prop_assert_eq!(before, list);
    }

    #[test]
    fn sessions_survive_json(
        title in "\\PC{0,20}", path in proptest::option::of("[a-z/._ ]{1,30}"),
        dirty: bool, pinned: bool, line in 0u32..10_000, col in 0u32..500, scroll in 0u32..10_000,
        n in 0usize..5,
    ) {
        let mut s = Session::default();
        for i in 0..n {
            let mut t = TabRecord::new_untitled(&title);
            t.path = path.clone();
            t.dirty = dirty;
            t.pinned = pinned;
            t.cursor = Cursor { line, column: col };
            t.scroll_line = scroll;
            if dirty { t.buffer_file = Some(TabRecord::buffer_file_name(&t.id)); }
            if i == 0 { s.active_tab = Some(t.id.clone()); }
            s.tabs.push(t);
        }
        let parsed = Session::parse(&s.to_json()).unwrap();
        prop_assert_eq!(parsed, s);
    }

    #[test]
    fn corrupt_session_json_is_an_error_not_a_panic(garbage in "\\PC{0,200}") {
        let _ = Session::parse(&garbage);
    }
}
