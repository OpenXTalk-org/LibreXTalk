//! The `.xtstack.html` text form (CLAUDE.md 5.17, rules SFH-1 to SFH-9).
//!
//! A stack file is a valid HTML5 page, so a browser opens it and version
//! control shows small, readable diffs. Each object is a custom element
//! named `xt-` plus its kind:
//!
//! ```html
//! <!doctype html>
//! <html>
//! <head>
//! <meta charset="utf-8">
//! <meta name="generator" content="libreXTalk">
//! <meta name="xt-format" content="1">
//! <title>Demo</title>
//! </head>
//! <body>
//! <xt-stack id="4" name="Demo">
//! <script type="text/x-xtalk">
//! on openStack
//!   put "hi"
//! end openStack
//! </script>
//!   <xt-background id="5"></xt-background>
//!   <xt-card id="6" xt-background="5">
//!     <xt-button id="7" name="OK" rect="10,10,90,34"></xt-button>
//!     <xt-field id="8" name="Notes">
//!       <p>line one</p>
//!       <p>line two</p>
//!     </xt-field>
//!   </xt-card>
//! </xt-stack>
//! </body>
//! </html>
//! ```
//!
//! The reader accepts what the writer writes plus reasonable hand edits:
//! any whitespace between elements, comments, single or double quotes or
//! none around attribute values, the usual character references, and other
//! HTML elements, which it skips (their `xt-` children still count).

use crate::{PartNode, XtError};

/// The version of the text form this crate writes. A reader refuses files
/// from a newer version rather than guess at them (SFH-9).
pub const FORMAT_VERSION: u32 = 1;

/// The kinds of object, as the engine names them. Element names are the
/// same words in lowercase, because HTML lowercases element names.
const KINDS: &[&str] = &[
    "stack",
    "background",
    "card",
    "group",
    "button",
    "field",
    "scrollbar",
    "image",
    "graphic",
    "part",
    "audioClip",
    "videoClip",
];

/// Attribute names that already mean something in HTML. A property with one
/// of these names is written with an `xt-` prefix, so `style="standard"` on
/// a button does not turn into a CSS style, and a custom property named
/// `onclick` never becomes a script a browser runs (SFH-4).
const HTML_ATTRIBUTES: &[&str] = &[
    "accesskey",
    "autocapitalize",
    "autofocus",
    "class",
    "contenteditable",
    "dir",
    "draggable",
    "enterkeyhint",
    "exportparts",
    "hidden",
    "id",
    "inert",
    "inputmode",
    "is",
    "itemid",
    "itemprop",
    "itemref",
    "itemscope",
    "itemtype",
    "lang",
    "nonce",
    "part",
    "popover",
    "slot",
    "spellcheck",
    "style",
    "tabindex",
    "title",
    "translate",
];

fn needs_prefix(name: &str) -> bool {
    name.starts_with("on") || HTML_ATTRIBUTES.contains(&name)
}

// ----- writing ---------------------------------------------------------------

/// Write a stack as an `.xtstack.html` page. The same tree always gives the
/// same text, byte for byte, so saving an unchanged stack changes nothing
/// in version control (SFH-1).
pub fn write(stack: &PartNode) -> String {
    let name = stack
        .props
        .iter()
        .find(|(k, _)| k == "name")
        .map(|(_, v)| v.as_str())
        .unwrap_or("");
    let mut out = String::new();
    out.push_str("<!doctype html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"generator\" content=\"libreXTalk\">\n");
    out.push_str(&format!(
        "<meta name=\"xt-format\" content=\"{FORMAT_VERSION}\">\n"
    ));
    out.push_str(&format!("<title>{}</title>\n", escape_text(name)));
    out.push_str("</head>\n<body>\n");
    write_node(stack, 0, &mut out);
    out.push_str("</body>\n</html>\n");
    out
}

fn write_node(node: &PartNode, depth: usize, out: &mut String) {
    let indent = "  ".repeat(depth);
    let tag = format!("xt-{}", node.kind.to_lowercase());
    let is_field = node.kind.eq_ignore_ascii_case("field");

    // Attributes in a fixed order: id, the background a card uses, the
    // name, then the other properties sorted by name (SFH-3).
    let mut attrs = vec![("id".to_string(), node.id.to_string())];
    if let Some(background) = node.background {
        attrs.push(("xt-background".to_string(), background.to_string()));
    }
    let mut props: Vec<&(String, String)> = node.props.iter().collect();
    props.sort_by(|a, b| (a.0 != "name", &a.0).cmp(&(b.0 != "name", &b.0)));
    let mut field_text = None;
    for (key, value) in props {
        let key = key.to_lowercase();
        if is_field && key == "text" {
            field_text = Some(value.as_str());
            continue;
        }
        let attr = if needs_prefix(&key) {
            format!("xt-{key}")
        } else {
            key
        };
        attrs.push((attr, value.clone()));
    }

    out.push_str(&indent);
    out.push('<');
    out.push_str(&tag);
    for (attr, value) in &attrs {
        out.push_str(&format!(" {attr}=\"{}\"", escape_attr(value)));
    }
    out.push('>');

    let has_body = !node.script.is_empty() || field_text.is_some() || !node.children.is_empty();
    if !has_body {
        out.push_str(&format!("</{tag}>\n"));
        return;
    }
    out.push('\n');
    if !node.script.is_empty() {
        write_script(&node.script, out);
    }
    if let Some(text) = field_text {
        // A field's text is one paragraph per line, so a browser shows it
        // and a diff shows which line changed (SFH-6).
        for line in text.split('\n') {
            out.push_str(&format!("{indent}  <p>{}</p>\n", escape_text(line)));
        }
    }
    for child in &node.children {
        write_node(child, depth + 1, out);
    }
    out.push_str(&format!("{indent}</{tag}>\n"));
}

/// A script goes in a `<script type="text/x-xtalk">` element, which a
/// browser neither shows nor runs. Its text is written exactly as it is,
/// starting at the left margin, because indenting it would change it. Text
/// in a script element can't be escaped, so a script that contains
/// `</script` or `<!--` goes in an inert `<template>` with its characters
/// escaped instead (SFH-5).
fn write_script(script: &str, out: &mut String) {
    let lower = script.to_lowercase();
    if lower.contains("</script") || lower.contains("<!--") {
        out.push_str("<template data-xt=\"script\">\n");
        out.push_str(&escape_text(script));
        out.push_str("\n</template>\n");
    } else {
        out.push_str("<script type=\"text/x-xtalk\">\n");
        out.push_str(script);
        out.push_str("\n</script>\n");
    }
}

fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

/// Attribute values are double-quoted. Line breaks and tabs are written as
/// character references so every element stays on one line (SFH-3).
fn escape_attr(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '"' => out.push_str("&quot;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            '\t' => out.push_str("&#9;"),
            _ => out.push(c),
        }
    }
    out
}

// ----- reading ---------------------------------------------------------------

/// Read an `.xtstack.html` page back into a stack tree (SFH-2).
pub fn read(text: &str) -> Result<PartNode, XtError> {
    let mut reader = Reader {
        text,
        pos: 0,
        open: Vec::new(),
        stack: None,
        in_paragraph: false,
    };
    reader.run()?;
    reader
        .stack
        .ok_or_else(|| XtError::new("This page has no <xt-stack> element, so it holds no stack"))
}

/// A start tag as read.
struct Tag {
    /// The element name, in lowercase.
    name: String,
    /// Attribute names in lowercase, values with references decoded.
    attrs: Vec<(String, String)>,
    /// True when the tag ended with `/>`.
    self_closing: bool,
}

/// An `xt-` element that has started but not yet ended.
struct Open {
    node: PartNode,
    tag: String,
    line: usize,
    /// For a field: its paragraphs so far, one per line of text.
    lines: Option<Vec<String>>,
}

struct Reader<'a> {
    text: &'a str,
    pos: usize,
    open: Vec<Open>,
    stack: Option<PartNode>,
    /// True between a field's `<p>` and its `</p>`. The paragraph being
    /// read is then the last line of the field on top of `open`.
    in_paragraph: bool,
}

impl<'a> Reader<'a> {
    /// The text not read yet. It borrows from the input (`'a`), not from
    /// the reader, so the reader can move on while holding it.
    fn rest(&self) -> &'a str {
        &self.text[self.pos..]
    }

    /// The line the reader is on, counting from 0 like the engine's errors.
    fn line(&self) -> usize {
        self.text[..self.pos].matches('\n').count()
    }

    fn error(&self, message: impl Into<String>) -> XtError {
        let mut e = XtError::new(message);
        e.line = Some(self.line());
        e
    }

    fn run(&mut self) -> Result<(), XtError> {
        while self.pos < self.text.len() {
            let rest = self.rest();
            if rest.starts_with("<!--") {
                let end = rest
                    .find("-->")
                    .ok_or_else(|| self.error("A comment (<!--) is never closed with -->"))?;
                self.pos += end + 3;
            } else if rest.starts_with("<!") || rest.starts_with("<?") {
                // `<!doctype html>` and the like
                let end = rest
                    .find('>')
                    .ok_or_else(|| self.error("A '<!' tag never ends"))?;
                self.pos += end + 1;
            } else if rest.starts_with("</") {
                self.end_tag()?;
            } else if rest.starts_with('<')
                && rest[1..].starts_with(|c: char| c.is_ascii_alphabetic())
            {
                self.start_tag()?;
            } else {
                // Text between tags. Only a field's paragraphs keep theirs.
                let end = rest.find('<').unwrap_or(rest.len());
                let text = decode_refs(&rest[..end]);
                self.pos += end;
                if let Some(paragraph) = self.open_paragraph() {
                    paragraph.push_str(&text);
                } else if !text.trim().is_empty() && !self.open.is_empty() {
                    return Err(self.error(format!(
                        "Unexpected text inside <{}>: \"{}\"",
                        self.open.last().map(|o| o.tag.as_str()).unwrap_or(""),
                        text.trim()
                    )));
                }
            }
        }
        if let Some(open) = self.open.last() {
            let mut e = XtError::new(format!("<{}> is never closed", open.tag));
            e.line = Some(open.line);
            return Err(e);
        }
        Ok(())
    }

    /// The paragraph being read, if the reader is inside a field's `<p>`.
    fn open_paragraph(&mut self) -> Option<&mut String> {
        if !self.in_paragraph {
            return None;
        }
        self.open.last_mut()?.lines.as_mut()?.last_mut()
    }

    fn start_tag(&mut self) -> Result<(), XtError> {
        let line = self.line();
        let Tag {
            name,
            attrs,
            self_closing,
        } = self.parse_tag()?;
        if name == "script" {
            return self.script_element(&attrs);
        }
        if name == "template" && attr(&attrs, "data-xt") == Some("script") {
            return self.template_script();
        }
        if name == "style" || name == "title" || name == "textarea" {
            // Raw text elements: skip their contents whole.
            self.skip_to_end_of(&name)?;
            return Ok(());
        }
        if name == "meta" && attr(&attrs, "name") == Some("xt-format") {
            let version = attr(&attrs, "content").unwrap_or("");
            match version.trim().parse::<u32>() {
                Ok(v) if v <= FORMAT_VERSION => {}
                Ok(v) => {
                    return Err(self.error(format!(
                        "This stack file uses format {v}, which is newer than this program understands ({FORMAT_VERSION})"
                    )))
                }
                Err(_) => return Err(self.error(format!("Unknown stack file format \"{version}\""))),
            }
            return Ok(());
        }
        if name == "p" {
            if let Some(lines) = self.open.last_mut().and_then(|o| o.lines.as_mut()) {
                lines.push(String::new());
                if self.in_paragraph {
                    return Err(self.error("A <p> inside a field starts before the last one ended"));
                }
                self.in_paragraph = true;
                return Ok(());
            }
        }
        if let Some(kind) = name.strip_prefix("xt-") {
            let kind = KINDS
                .iter()
                .find(|k| k.eq_ignore_ascii_case(kind))
                .ok_or_else(|| self.error(format!("Unknown kind of object: <{name}>")))?;
            let node = self.node_from(kind, &attrs)?;
            let lines = (*kind == "field").then(Vec::new);
            if self.stack.is_some() && self.open.is_empty() {
                return Err(self.error("A stack file holds one stack; this one has a second <xt-stack> or an element after it"));
            }
            self.open.push(Open {
                node,
                tag: name,
                line,
                lines,
            });
            if self_closing {
                self.close_top()?;
            }
            return Ok(());
        }
        // Any other element (html, body, div, ...) is skipped, and so is its
        // end tag; its contents are still read.
        Ok(())
    }

    fn node_from(&self, kind: &str, attrs: &[(String, String)]) -> Result<PartNode, XtError> {
        let mut node = PartNode {
            kind: kind.to_string(),
            ..PartNode::default()
        };
        let mut has_id = false;
        for (name, value) in attrs {
            match name.as_str() {
                "id" => {
                    node.id = value.trim().parse().map_err(|_| {
                        self.error(format!(
                            "The id of <xt-{}> must be a whole number, not \"{value}\"",
                            kind.to_lowercase()
                        ))
                    })?;
                    has_id = true;
                }
                "xt-background" => {
                    node.background = Some(value.trim().parse().map_err(|_| {
                        self.error(format!("xt-background must be a card id, not \"{value}\""))
                    })?);
                }
                _ => {
                    let prop = match name.strip_prefix("xt-") {
                        Some(p) if needs_prefix(p) => p,
                        Some(_) => {
                            return Err(self.error(format!(
                                "Unknown attribute {name} on <xt-{}>",
                                kind.to_lowercase()
                            )))
                        }
                        None if needs_prefix(name) => {
                            return Err(self.error(format!(
                                "The attribute {name} means something else in HTML; write the property as xt-{name}"
                            )))
                        }
                        None => name.as_str(),
                    };
                    node.props.push((prop.to_string(), value.clone()));
                }
            }
        }
        if !has_id {
            return Err(self.error(format!("<xt-{}> has no id", kind.to_lowercase())));
        }
        Ok(node)
    }

    fn end_tag(&mut self) -> Result<(), XtError> {
        let start = self.pos;
        let rest = self.rest();
        let end = rest
            .find('>')
            .ok_or_else(|| self.error("An end tag never ends with '>'"))?;
        let name = rest[2..end].trim().to_lowercase();
        self.pos = start + end + 1;
        if name == "p" && self.in_paragraph {
            self.in_paragraph = false;
            return Ok(());
        }
        if name.starts_with("xt-") {
            match self.open.last() {
                Some(open) if open.tag == name => self.close_top(),
                Some(open) => Err(self.error(format!(
                    "</{name}> here, but <{}> (line {}) is still open",
                    open.tag,
                    open.line + 1
                ))),
                None => Err(self.error(format!("</{name}> without a matching <{name}>"))),
            }
        } else {
            Ok(())
        }
    }

    /// The element on top ends: finish it and add it to its parent.
    fn close_top(&mut self) -> Result<(), XtError> {
        if self.in_paragraph {
            return Err(self.error("A field's <p> is never closed"));
        }
        let Some(mut open) = self.open.pop() else {
            return Ok(());
        };
        if let Some(lines) = open.lines.take() {
            if !lines.is_empty() {
                open.node.props.push(("text".to_string(), lines.join("\n")));
            }
        }
        open.node.props.sort();
        match self.open.last_mut() {
            Some(parent) => parent.node.children.push(open.node),
            None => {
                if open.node.kind != "stack" {
                    return Err(self.error(format!("<{}> must be inside an <xt-stack>", open.tag)));
                }
                self.stack = Some(open.node);
            }
        }
        Ok(())
    }

    fn script_element(&mut self, attrs: &[(String, String)]) -> Result<(), XtError> {
        let start = self.pos;
        let end = find_ci(self.rest(), "</script")
            .ok_or_else(|| self.error("A <script> is never closed"))?;
        let body = self.text[start..start + end].to_string();
        self.pos = start + end;
        self.skip_past('>')?;
        let kind = attr(attrs, "type").unwrap_or("").to_lowercase();
        if kind != "text/x-xtalk" {
            // A browser script, such as a future preview: not part of the stack.
            return Ok(());
        }
        self.set_script(trim_one_newline(&body).to_string())
    }

    fn template_script(&mut self) -> Result<(), XtError> {
        let start = self.pos;
        let end = find_ci(self.rest(), "</template")
            .ok_or_else(|| self.error("A <template> is never closed"))?;
        let body = decode_refs(&self.text[start..start + end]);
        self.pos = start + end;
        self.skip_past('>')?;
        self.set_script(trim_one_newline(&body).to_string())
    }

    fn set_script(&mut self, script: String) -> Result<(), XtError> {
        let error = self.error("A script must be inside the object it belongs to");
        let open = self.open.last_mut().ok_or(error)?;
        if !open.node.script.is_empty() {
            let message = format!("<{}> has two scripts", open.tag);
            return Err(self.error(message));
        }
        open.node.script = script;
        Ok(())
    }

    fn skip_to_end_of(&mut self, name: &str) -> Result<(), XtError> {
        let end = find_ci(self.rest(), &format!("</{name}"))
            .ok_or_else(|| self.error(format!("<{name}> is never closed")))?;
        self.pos += end;
        self.skip_past('>')
    }

    fn skip_past(&mut self, c: char) -> Result<(), XtError> {
        let end = self
            .rest()
            .find(c)
            .ok_or_else(|| self.error(format!("Expected '{c}'")))?;
        self.pos += end + 1;
        Ok(())
    }

    /// Read `<name attr="value" ...>`.
    fn parse_tag(&mut self) -> Result<Tag, XtError> {
        self.pos += 1; // '<'
        let name = self.take_name();
        let mut attrs = Vec::new();
        loop {
            self.skip_space();
            let rest = self.rest();
            if rest.starts_with("/>") {
                self.pos += 2;
                return Ok(Tag {
                    name,
                    attrs,
                    self_closing: true,
                });
            }
            if rest.starts_with('>') {
                self.pos += 1;
                return Ok(Tag {
                    name,
                    attrs,
                    self_closing: false,
                });
            }
            if rest.is_empty() {
                return Err(self.error(format!("The tag <{name} never ends with '>'")));
            }
            let attr_name = self.take_name();
            if attr_name.is_empty() {
                return Err(self.error(format!("Unexpected character in the tag <{name}>")));
            }
            self.skip_space();
            let value = if self.rest().starts_with('=') {
                self.pos += 1;
                self.skip_space();
                self.take_value()?
            } else {
                String::new()
            };
            attrs.push((attr_name, value));
        }
    }

    fn take_name(&mut self) -> String {
        let rest = self.rest();
        let end = rest
            .find(|c: char| c.is_whitespace() || c == '>' || c == '/' || c == '=')
            .unwrap_or(rest.len());
        self.pos += end;
        rest[..end].to_lowercase()
    }

    fn take_value(&mut self) -> Result<String, XtError> {
        let rest = self.rest();
        let quote = rest.chars().next();
        if let Some(q @ ('"' | '\'')) = quote {
            let end = rest[1..]
                .find(q)
                .ok_or_else(|| self.error("An attribute value is never closed"))?;
            let raw = &rest[1..1 + end];
            self.pos += end + 2;
            Ok(decode_refs(raw))
        } else {
            let end = rest
                .find(|c: char| c.is_whitespace() || c == '>')
                .unwrap_or(rest.len());
            let raw = &rest[..end];
            self.pos += end;
            Ok(decode_refs(raw))
        }
    }

    fn skip_space(&mut self) {
        let rest = self.rest();
        let n = rest.len() - rest.trim_start().len();
        self.pos += n;
    }
}

fn attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

/// The writer puts a line break after the opening tag and before the
/// closing one; take exactly those back off, and nothing more.
fn trim_one_newline(body: &str) -> &str {
    let body = body
        .strip_prefix("\r\n")
        .or_else(|| body.strip_prefix('\n'))
        .unwrap_or(body);
    body.strip_suffix("\r\n")
        .or_else(|| body.strip_suffix('\n'))
        .unwrap_or(body)
}

/// Find `needle` (ASCII) in `haystack`, ignoring case.
fn find_ci(haystack: &str, needle: &str) -> Option<usize> {
    let hay = haystack.as_bytes();
    let pin = needle.as_bytes();
    (0..hay.len().saturating_sub(pin.len() - 1))
        .find(|&i| hay[i..i + pin.len()].eq_ignore_ascii_case(pin))
}

/// Turn character references such as `&amp;` and `&#10;` back into
/// characters. An unknown reference is left as it is.
fn decode_refs(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(semi) = rest[1..].find(';').map(|i| i + 1).filter(|&i| i <= 12) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..semi];
        let decoded = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            _ => {
                if let Some(hex) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
                    u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
                } else if let Some(dec) = name.strip_prefix('#') {
                    dec.parse::<u32>().ok().and_then(char::from_u32)
                } else {
                    None
                }
            }
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}
