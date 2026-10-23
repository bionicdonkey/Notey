//! Find and replace: plain, regex, scopes, counters and history.
use super::support::*;
use gtk::prelude::*;
use notey::ui::doc::Doc;
use notey::ui::window::NoteyWindow;
use std::rc::Rc;

/// Opens the bar and sets the query the way typing and clicking would.
fn query(win: &NoteyWindow, pattern: &str, replace: &str, regex: bool, case: bool, word: bool) {
    win.open_find(true);
    win.find.regex.set_active(regex);
    win.find.case.set_active(case);
    win.find.word.set_active(word);
    win.find.find_entry.set_text(pattern);
    win.find.replace_entry.set_text(replace);
    pump_for(260); // the query is debounced
}

fn doc(win: &NoteyWindow, text: &str) -> Rc<Doc> {
    let d = win.new_tab();
    d.set_text(text, false);
    d
}

fn selected(d: &Doc) -> String {
    d.buffer.selection_bounds().map(|(a, b)| d.buffer.text(&a, &b, false).to_string()).unwrap_or_default()
}

pub fn scenarios() -> Vec<Scenario> {
    vec![
        scenario("find_plain_replace_all_respects_case_and_whole_word", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = doc(&win, "cat Cat concat\n");
            query(&win, "cat", "dog", false, true, false);
            win.replace_all();
            assert_eq!(d.text(), "dog Cat condog\n");
            d.set_text("cat Cat concat\n", false);
            query(&win, "cat", "dog", false, false, true);
            win.replace_all();
            assert_eq!(d.text(), "dog dog concat\n", "whole words, either case");
        }),
        scenario("find_plain_mode_treats_regex_characters_literally", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = doc(&win, "a.b*c (x) a+b\n");
            query(&win, "a.b*c", "<1>", false, true, false);
            win.replace_all();
            assert_eq!(d.text(), "<1> (x) a+b\n");
            query(&win, "(x)", "[y]", false, true, false);
            win.replace_all();
            assert_eq!(d.text(), "<1> [y] a+b\n");
        }),
        scenario("find_regex_groups_work_with_dollar_and_backslash_syntax", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = doc(&win, "user@example admin@site\n");
            query(&win, r"(\w+)@(\w+)", "$2 at $1", true, true, false);
            win.replace_all();
            assert_eq!(d.text(), "example at user site at admin\n");
            d.set_text("user@example\n", false);
            query(&win, r"(\w+)@(\w+)", r"\2:\1", true, true, false);
            win.replace_all();
            assert_eq!(d.text(), "example:user\n");
            d.set_text("price 5\n", false);
            query(&win, r"(\d)", "$$$1", true, true, false);
            win.replace_all();
            assert_eq!(d.text(), "price $5\n", "$$ is a literal dollar");
        }),
        scenario("find_regex_anchors_match_at_every_line", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = doc(&win, "a\nb\nc");
            query(&win, "^", "> ", true, true, false);
            win.replace_all();
            assert_eq!(d.text(), "> a\n> b\n> c");
            query(&win, "$", ";", true, true, false);
            win.replace_all();
            assert_eq!(d.text(), "> a;\n> b;\n> c;");
        }),
        scenario("find_regex_case_insensitive_and_lookaround", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = doc(&win, "Foo foo FOO foobar\n");
            query(&win, r"foo(?!bar)", "x", true, false, false);
            win.replace_all();
            assert_eq!(d.text(), "x x x foobar\n");
        }),
        scenario("find_an_invalid_regex_is_flagged_not_fatal", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = doc(&win, "text\n");
            query(&win, "([unclosed", "", true, true, false);
            assert!(win.find.find_entry.has_css_class("error"));
            assert!(win.find.error.is_visible() || win.find.error.get_visible());
            win.replace_all();
            assert_eq!(d.text(), "text\n", "nothing replaced");
            query(&win, "text", "", true, true, false);
            assert!(!win.find.find_entry.has_css_class("error"), "recovers when fixed");
        }),
        scenario("find_the_counter_reports_matches_and_position", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = doc(&win, "ab ab ab\nab\n");
            query(&win, "ab", "", false, true, false);
            pump_until("the count", || win.find.count.label().contains("found") || win.find.count.label().contains(" of "));
            assert!(win.find.count.label().contains('4'), "{}", win.find.count.label());
            win.find_step(true);
            pump_for(200);
            assert_eq!(selected(&d), "ab");
            assert!(win.find.count.label().starts_with("1 of 4") || win.find.count.label().starts_with("2 of 4"), "{}", win.find.count.label());
            query(&win, "zzz", "", false, true, false);
            pump_until("no results", || win.find.count.label() == "No results");
        }),
        scenario("find_next_and_previous_walk_the_matches_and_wrap", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = doc(&win, "x1 x2 x3");
            query(&win, "x", "", false, true, false);
            d.set_cursor(notey::core::session::Cursor { line: 0, column: 0 });
            let mut offsets = Vec::new();
            for _ in 0..4 {
                win.find_step(true);
                pump_for(120);
                offsets.push(d.buffer.selection_bounds().unwrap().0.offset());
            }
            assert_eq!(offsets, vec![0, 3, 6, 0], "wraps back to the first");
            win.find_step(false);
            pump_for(120);
            assert_eq!(d.buffer.selection_bounds().unwrap().0.offset(), 6);
        }),
        scenario("find_replace_one_replaces_the_selected_match_then_advances", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = doc(&win, "one one one");
            query(&win, "one", "two", false, true, false);
            d.set_cursor(notey::core::session::Cursor { line: 0, column: 0 });
            win.find_step(true);
            pump_for(120);
            win.replace_one();
            pump_for(200);
            assert_eq!(d.text(), "two one one");
            assert_eq!(selected(&d), "one", "advanced to the next match");
            win.replace_one();
            pump_for(200);
            assert_eq!(d.text(), "two two one");
        }),
        scenario("find_replace_one_does_nothing_when_the_selection_is_not_a_match", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = doc(&win, "alpha beta");
            query(&win, "beta", "X", false, true, false);
            d.buffer.select_range(&d.buffer.iter_at_offset(0), &d.buffer.iter_at_offset(3));
            win.replace_one();
            pump_for(200);
            assert_eq!(d.text(), "alpha beta");
        }),
        scenario("find_in_selection_only_touches_the_selection", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = doc(&win, "a a a a");
            d.buffer.select_range(&d.buffer.iter_at_offset(2), &d.buffer.iter_at_offset(5)); // the middle "a a"
            win.open_find(true);
            win.find.scope.set_selected(1);
            pump_for(50);
            win.find.find_entry.set_text("a");
            win.find.replace_entry.set_text("b");
            pump_for(260);
            win.replace_all();
            assert_eq!(d.text(), "a b b a");
        }),
        scenario("find_all_tabs_lists_matches_from_every_tab", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let a = doc(&win, "needle one\nplain\n");
            let b = doc(&win, "second needle needle\n");
            win.open_find(false);
            win.find.scope.set_selected(2);
            win.find.find_entry.set_text("needle");
            pump_for(300);
            assert!(win.find.results_scroller.get_visible());
            let rows = (0..).take_while(|i| win.find.results.row_at_index(*i).is_some()).count();
            assert_eq!(rows, 3);
            assert!(win.find.count.label().contains("3 in all tabs"), "{}", win.find.count.label());
            // Stepping jumps to the tab that holds the match.
            win.tab_view.set_selected_page(&win.page_of(&a).unwrap());
            win.find_step(true);
            win.find_step(true);
            pump_for(100);
            assert!(Rc::ptr_eq(&win.current_doc().unwrap(), &b) || Rc::ptr_eq(&win.current_doc().unwrap(), &a));
            assert!(!selected(&win.current_doc().unwrap()).is_empty());
        }),
        scenario("find_an_empty_query_clears_everything", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = doc(&win, "text");
            query(&win, "text", "", false, true, false);
            query(&win, "", "", false, true, false);
            assert_eq!(win.find.count.label(), "");
            win.replace_all();
            assert_eq!(d.text(), "text");
        }),
        scenario("find_options_and_history_are_remembered", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            doc(&win, "hello world");
            query(&win, "hello", "", true, true, true);
            win.find.find_entry.emit_activate(); // Enter in the entry records the query and steps
            win.close_find();
            let s = win.settings.borrow().clone();
            assert!(s.search_regex && s.search_case_sensitive && s.search_whole_word);
            assert_eq!(s.search_history.first().map(String::as_str), Some("hello"));
            // A second window starts with the same options.
            let win2 = env.window_with(&sb, "session2", s);
            assert!(win2.find.regex.is_active() && win2.find.case.is_active() && win2.find.word.is_active());
        }),
        scenario("find_history_can_be_switched_off", |env| {
            let sb = env.sandbox();
            let settings = notey::settings::Settings { remember_search_history: false, ..Default::default() };
            let win = env.window_with(&sb, "session", settings);
            doc(&win, "hello");
            query(&win, "hello", "", false, true, false);
            win.find_step(true);
            assert!(win.settings.borrow().search_history.is_empty());
        }),
        scenario("find_closing_the_bar_removes_highlighting_and_hides_it", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let d = doc(&win, "abc abc");
            query(&win, "abc", "", false, true, false);
            assert!(d.search.is_highlight());
            win.close_find();
            assert!(!d.search.is_highlight());
            assert!(!win.find.root.get_visible());
        }),
        scenario("find_switching_tabs_searches_the_new_tab", |env| {
            let sb = env.sandbox();
            let win = env.window(&sb);
            let a = doc(&win, "apple apple");
            let b = doc(&win, "apple");
            query(&win, "apple", "", false, true, false);
            pump_until("count for b", || win.find.count.label().contains("1 found") || win.find.count.label().contains("1 of 1"));
            win.tab_view.set_selected_page(&win.page_of(&a).unwrap());
            pump_until("count for a", || win.find.count.label().contains('2'));
            assert!(a.search.is_highlight() && !b.search.is_highlight());
        }),
    ]
}
