//! The web engine's JSON answers, tested as ordinary Rust.

use crate::Engine;
use xt_core::HostReply;

/// A fixed page clock: 2026-11-28 00:00:00 UTC, in milliseconds.
const NOW: f64 = 1_795_996_800_000.0;

#[test]
fn a_script_runs_and_its_output_comes_back() {
    let mut e = Engine::new();
    assert_eq!(
        e.load("put \"hello\"\nput 1 + 2"),
        r#"{"status":"loaded","messages":[]}"#
    );
    assert_eq!(
        e.run(NOW),
        r#"{"status":"finished","messages":["hello","3"]}"#
    );
    // Messages are handed over once.
    assert_eq!(e.run(NOW), r#"{"status":"finished","messages":[]}"#);
}

#[test]
fn text_is_escaped_as_json() {
    let mut e = Engine::new();
    e.load("put quote & \"a\\b\" & tab & return & \"end\"");
    assert_eq!(
        e.run(NOW),
        r#"{"status":"finished","messages":["\"a\\b\t\nend"]}"#
    );
}

#[test]
fn a_wait_comes_back_as_sleeping() {
    let mut e = Engine::new();
    e.load("put 1\nwait 30 ticks\nput 2");
    assert_eq!(
        e.run(NOW),
        r#"{"status":"sleeping","messages":["1"],"ms":500}"#
    );
}

#[test]
fn a_dialog_waits_for_the_page() {
    let mut e = Engine::new();
    e.load("answer \"Sure?\" with \"Yes\" or \"No\"\nput it");
    assert_eq!(
        e.run(NOW),
        r#"{"status":"waiting","messages":[],"request":{"kind":"answer","prompt":"Sure?","buttons":["Yes","No"]}}"#
    );
    assert_eq!(
        e.reply(HostReply::Button(1)),
        r#"{"status":"replied","messages":[]}"#
    );
    assert_eq!(e.run(NOW), r#"{"status":"finished","messages":["No"]}"#);
}

#[test]
fn every_request_is_described() {
    let mut e = Engine::new();
    e.load(
        r#"
ask "Name?" with "Ada"
answer file "Open:" of type "txt"
save this stack as "s.xtstack.html"
open stack "t.xtstack.html"
"#,
    );
    let first = e.run(NOW);
    assert!(first.contains(r#""request":{"kind":"ask","prompt":"Name?","default":"Ada"}"#));
    e.reply(HostReply::Text(None));
    assert!(e.run(NOW).contains(
        r#""request":{"kind":"dialog","form":"answer file","prompt":"Open:","default":"","types":["txt"]}"#
    ));
    e.reply(HostReply::Text(Some("/a.txt".to_string())));
    let save = e.run(NOW);
    assert!(save
        .contains(r#""request":{"kind":"save","path":"s.xtstack.html","text":"<!doctype html>\n"#));
    assert!(save.contains(r#"<xt-stack id=\"1\" name=\"Untitled\">"#));
    e.reply(HostReply::Saved(Ok(())));
    assert!(e
        .run(NOW)
        .contains(r#""request":{"kind":"open","path":"t.xtstack.html"}"#));
}

#[test]
fn an_opened_file_becomes_a_stack() {
    let mut e = Engine::new();
    e.load("open stack \"t.xtstack.html\"\nput the name of this stack");
    e.run(NOW);
    let file = r#"<xt-stack id="7" name="From the page"><xt-card id="1"></xt-card></xt-stack>"#;
    assert_eq!(
        e.reply_opened(file),
        r#"{"status":"replied","messages":[]}"#
    );
    assert_eq!(
        e.run(NOW),
        r#"{"status":"finished","messages":["From the page"]}"#
    );
}

#[test]
fn a_bad_file_is_an_error_the_script_can_catch() {
    let mut e = Engine::new();
    e.load("try\n  open stack \"t\"\ncatch err\n  put \"caught\"\nend try");
    e.run(NOW);
    e.reply_opened("not a stack");
    assert_eq!(e.run(NOW), r#"{"status":"finished","messages":["caught"]}"#);
}

#[test]
fn errors_say_what_and_where() {
    let mut e = Engine::new();
    assert_eq!(
        e.load("put 1\nput (2"),
        r#"{"status":"error","messages":[],"error":{"message":"Expected a closing parenthesis here","line":2}}"#
    );
    e.load("put 1\nput 1 + \"x\"");
    let answer = e.run(NOW);
    assert!(answer.starts_with(r#"{"status":"error","messages":["1"],"error":{"message":"#));
    assert!(answer.ends_with(r#""line":2}}"#));
}

#[test]
fn a_reply_when_nothing_waits_is_an_error() {
    let mut e = Engine::new();
    assert!(e
        .reply(HostReply::Button(0))
        .contains("Nothing is waiting for a reply"));
}

#[test]
fn the_time_comes_from_the_page() {
    let mut e = Engine::new();
    e.load("put the seconds\nput the milliseconds");
    assert_eq!(
        e.run(NOW),
        r#"{"status":"finished","messages":["1795996800","1795996800000"]}"#
    );
}
