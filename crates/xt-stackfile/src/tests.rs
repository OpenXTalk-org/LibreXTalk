//! Tests for the stack file formats.

use crate::{decode, encode, format_for_path, html, Format, PartNode};

fn node(kind: &str, id: i64, props: &[(&str, &str)]) -> PartNode {
    let mut props: Vec<(String, String)> = props
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    props.sort();
    PartNode {
        kind: kind.to_string(),
        id,
        props,
        ..PartNode::default()
    }
}

/// A small stack: a background, a card with a button and a field, and a clip.
fn demo() -> PartNode {
    let mut stack = node("stack", 4, &[("name", "Demo")]);
    stack.script = "on openStack\n  put \"hi\"\nend openStack".to_string();
    stack.children.push(node("background", 5, &[]));
    let mut card = node("card", 6, &[]);
    card.background = Some(5);
    let mut button = node("button", 7, &[("name", "OK"), ("rect", "10,10,90,34")]);
    button.script = "on mouseUp\n  beep\nend mouseUp".to_string();
    card.children.push(button);
    card.children.push(node(
        "field",
        8,
        &[("name", "Notes"), ("text", "line one\nline two")],
    ));
    stack.children.push(card);
    stack
        .children
        .push(node("audioClip", 9, &[("name", "beep")]));
    stack
}

#[test]
fn the_text_form_is_a_readable_html_page() {
    let expected = r#"<!doctype html>
<html>
<head>
<meta charset="utf-8">
<meta name="generator" content="libreXTalk">
<meta name="xt-format" content="1">
<title>Demo</title>
</head>
<body>
<xt-stack id="4" name="Demo">
<script type="text/x-xtalk">
on openStack
  put "hi"
end openStack
</script>
  <xt-background id="5"></xt-background>
  <xt-card id="6" xt-background="5">
    <xt-button id="7" name="OK" rect="10,10,90,34">
<script type="text/x-xtalk">
on mouseUp
  beep
end mouseUp
</script>
    </xt-button>
    <xt-field id="8" name="Notes">
      <p>line one</p>
      <p>line two</p>
    </xt-field>
  </xt-card>
  <xt-audioclip id="9" name="beep"></xt-audioclip>
</xt-stack>
</body>
</html>
"#;
    assert_eq!(html::write(&demo()), expected);
}

#[test]
fn a_stack_survives_writing_and_reading() {
    let stack = demo();
    let text = html::write(&stack);
    assert_eq!(html::read(&text).expect("should read"), stack);
    // Writing what was read gives the same bytes again (SFH-1).
    assert_eq!(html::write(&html::read(&text).unwrap()), text);
}

#[test]
fn awkward_text_survives() {
    let mut stack = node(
        "stack",
        1,
        &[
            ("name", "Tom & \"Jerry\" <3"),
            ("note", "two\nlines\tand a tab\r\n"),
        ],
    );
    stack.script =
        "on test\n  -- a comment with </script> and <!-- in it\n  put \"&amp;\"\nend test\n"
            .to_string();
    stack.children.push(node("background", 2, &[]));
    let mut card = node("card", 3, &[]);
    card.background = Some(2);
    card.children.push(node(
        "field",
        4,
        &[("text", "  indented <b>not bold</b> & more\n\n\ttab\n")],
    ));
    stack.children.push(card);

    let text = html::write(&stack);
    // The script with `</script` in it goes in an inert template.
    assert!(text.contains("<template data-xt=\"script\">"));
    assert!(text.contains("&lt;/script&gt;"));
    assert!(text.contains("note=\"two&#10;lines&#9;and a tab&#13;&#10;\""));
    assert_eq!(html::read(&text).expect("should read"), stack);
}

#[test]
fn empty_lines_and_empty_text_are_kept_apart() {
    for text in ["", "\n", "a\n", "\na", "\n\n"] {
        let mut stack = node("stack", 1, &[]);
        let mut card = node("card", 2, &[]);
        let props: &[(&str, &str)] = if text.is_empty() {
            &[]
        } else {
            &[("text", text)]
        };
        card.children.push(node("field", 3, props));
        stack.children.push(card);
        let back = html::read(&html::write(&stack)).expect("should read");
        assert_eq!(back, stack, "field text {text:?}");
    }
}

#[test]
fn html_attribute_names_get_a_prefix() {
    // A button's `style` is not a CSS style, and a custom property named
    // `onclick` must never become browser script (SFH-4).
    let mut stack = node("stack", 1, &[]);
    let mut card = node("card", 2, &[]);
    card.children.push(node(
        "button",
        3,
        &[
            ("style", "checkbox"),
            ("onclick", "alert(1)"),
            ("title", "t"),
        ],
    ));
    stack.children.push(card);
    let text = html::write(&stack);
    assert!(text.contains("xt-onclick=\"alert(1)\""));
    assert!(text.contains("xt-style=\"checkbox\""));
    assert!(text.contains("xt-title=\"t\""));
    assert!(!text.contains(" onclick="));
    assert_eq!(html::read(&text).unwrap(), stack);

    // A hand-written `onclick` or `style` attribute is refused, not obeyed.
    let err = html::read(r#"<xt-stack id="1" onclick="x"></xt-stack>"#).unwrap_err();
    assert!(err.message.contains("write the property as xt-onclick"));
}

#[test]
fn hand_edits_are_accepted() {
    let text = r#"
<!-- edited by hand -->
<!DOCTYPE html>
<XT-STACK ID=1 name='Hand made'>
  <div class="anything">
    <xt-card id="2"><xt-button id='3' name="A &amp; B" hilite=true /></xt-card>
  </div>
  <xt-card id="4">
    <xt-field id="5"><p>one <b>bold</b> word</p>
    <p>two</p></xt-field>
  </xt-card>
</XT-STACK>
"#;
    let stack = html::read(text).expect("should read");
    assert_eq!(
        stack.props,
        vec![("name".to_string(), "Hand made".to_string())]
    );
    let button = &stack.children[0].children[0];
    assert_eq!(button.kind, "button");
    assert_eq!(
        button.props,
        vec![
            ("hilite".to_string(), "true".to_string()),
            ("name".to_string(), "A & B".to_string())
        ]
    );
    let field = &stack.children[1].children[0];
    assert_eq!(
        field.props,
        vec![("text".to_string(), "one bold word\ntwo".to_string())]
    );
}

#[test]
fn mistakes_are_reported_with_a_line() {
    let cases = [
        (
            "<xt-stack id=\"1\">\n<xt-card id=\"2\">\n</xt-stack>",
            "still open",
            2,
        ),
        (
            "<xt-stack id=\"1\">\n<xt-widgetx id=\"2\"></xt-widgetx>",
            "Unknown kind",
            1,
        ),
        ("<xt-stack id=\"1\">\n<xt-card></xt-card>", "has no id", 1),
        ("<xt-stack id=\"one\"></xt-stack>", "whole number", 0),
        (
            "<xt-stack id=\"1\">\n\n<xt-card id=\"2\">",
            "never closed",
            2,
        ),
        ("<xt-card id=\"2\"></xt-card>", "inside an <xt-stack>", 0),
        (
            "<meta name=\"xt-format\" content=\"99\"><xt-stack id=\"1\"></xt-stack>",
            "newer",
            0,
        ),
        (
            "<xt-stack id=\"1\">\nstray words</xt-stack>",
            "Unexpected text",
            1,
        ),
    ];
    for (text, message, line) in cases {
        let err = html::read(text).expect_err(text);
        assert!(err.message.contains(message), "{text}: {}", err.message);
        assert_eq!(err.line, Some(line), "{text}: {}", err.message);
    }
    assert!(html::read("<p>no stack</p>")
        .unwrap_err()
        .message
        .contains("no <xt-stack>"));
}

#[test]
fn browser_scripts_are_not_stack_scripts() {
    let text = r#"<xt-stack id="1">
<script>console.log("preview")</script>
<script type="text/x-xtalk">
on a
end a
</script>
</xt-stack>"#;
    assert_eq!(html::read(text).unwrap().script, "on a\nend a");
}

#[test]
fn the_extension_picks_the_format() {
    assert_eq!(format_for_path("a.xtstack.html"), Some(Format::Html));
    assert_eq!(format_for_path("A.XTSTACK.HTML"), Some(Format::Html));
    assert_eq!(format_for_path("a.xtstack"), Some(Format::Binary));
    assert_eq!(format_for_path("a.html"), None);
    let stack = demo();
    assert!(encode("x.xtstack", &stack)
        .unwrap_err()
        .message
        .contains("not built yet"));
    assert!(encode("notes.txt", &stack)
        .unwrap_err()
        .message
        .contains("notes.txt.xtstack.html"));
    let bytes = encode("x.xtstack.html", &stack).unwrap();
    // The contents decide how a file is read, so a renamed file still opens.
    assert_eq!(decode(&bytes).unwrap(), stack);
    assert!(decode(b"REVO7000")
        .unwrap_err()
        .message
        .contains("not a libreXTalk"));
}
