//! The object model: stacks, backgrounds, cards, groups, and controls.
//!
//! Objects live in an arena and are named by `ObjId`, which carries a
//! generation so a reference to a deleted object is detected, not reused.
//! This file holds only data and tree logic. Messages, scripts, and the
//! commands that act on objects are in the VM.

use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::ObjKind;
use crate::compile::Handler;
use crate::error::XtError;
use crate::interp::{format_number, to_number};

/// A handle to an object in the arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ObjId {
    index: usize,
    gen: u32,
}

pub struct Object {
    pub kind: ObjKind,
    /// The id scripts see. Unique and never reused.
    pub id: i64,
    pub parent: Option<ObjId>,
    pub children: Vec<ObjId>,
    /// For a card: the background it uses.
    pub background: Option<ObjId>,
    /// All properties except the computed ones, with lowercase keys.
    pub props: HashMap<String, String>,
    /// The handlers of this object's script, by lowercase name.
    pub handlers: HashMap<String, Rc<Handler>>,
}

struct Slot {
    gen: u32,
    obj: Option<Object>,
}

#[derive(Default)]
pub struct World {
    slots: Vec<Slot>,
    free: Vec<usize>,
    by_id: HashMap<i64, ObjId>,
    next_id: i64,
    pub stacks: Vec<ObjId>,
    pub current_stack: Option<ObjId>,
    pub current_card: Option<ObjId>,
}

// ----- kinds and prototypes -------------------------------------------------

pub fn kind_name(kind: ObjKind) -> &'static str {
    match kind {
        ObjKind::Stack => "stack",
        ObjKind::Background => "background",
        ObjKind::Card => "card",
        ObjKind::Group => "group",
        ObjKind::Button => "button",
        ObjKind::Field => "field",
        ObjKind::Scrollbar => "scrollbar",
        ObjKind::Image => "image",
        ObjKind::Graphic => "graphic",
    }
}

/// Recognize a kind of object by its name. Returns the kind and whether the
/// word was a plural. Short forms such as `btn` and `cd` are accepted.
pub fn kind_of_word(word: &str) -> Option<(ObjKind, bool)> {
    let pair = match word.to_lowercase().as_str() {
        "stack" => (ObjKind::Stack, false),
        "stacks" => (ObjKind::Stack, true),
        "background" | "bkgnd" | "bg" => (ObjKind::Background, false),
        "backgrounds" | "bkgnds" | "bgs" => (ObjKind::Background, true),
        "card" | "cd" => (ObjKind::Card, false),
        "cards" | "cds" => (ObjKind::Card, true),
        "group" => (ObjKind::Group, false),
        "groups" => (ObjKind::Group, true),
        "button" | "btn" => (ObjKind::Button, false),
        "buttons" | "btns" => (ObjKind::Button, true),
        "field" | "fld" => (ObjKind::Field, false),
        "fields" | "flds" => (ObjKind::Field, true),
        "scrollbar" => (ObjKind::Scrollbar, false),
        "scrollbars" => (ObjKind::Scrollbar, true),
        "image" => (ObjKind::Image, false),
        "images" => (ObjKind::Image, true),
        "graphic" => (ObjKind::Graphic, false),
        "graphics" => (ObjKind::Graphic, true),
        _ => return None,
    };
    Some(pair)
}

struct PropDef {
    name: &'static str,
    default: &'static str,
}

const fn pd(name: &'static str, default: &'static str) -> PropDef {
    PropDef { name, default }
}

const STACK_PROPS: &[PropDef] = &[
    pd("rect", "100,100,612,442"),
    pd("visible", "true"),
    pd("resizable", "true"),
    pd("cantdelete", "false"),
];
const BACKGROUND_PROPS: &[PropDef] = &[pd("cantdelete", "false")];
const CARD_PROPS: &[PropDef] = &[pd("cantdelete", "false"), pd("marked", "false")];
const GROUP_PROPS: &[PropDef] = &[pd("rect", "0,0,100,100"), pd("visible", "true")];
const BUTTON_PROPS: &[PropDef] = &[
    pd("rect", "0,0,100,24"),
    pd("visible", "true"),
    pd("enabled", "true"),
    pd("label", ""),
    pd("style", "standard"),
    pd("hilite", "false"),
    pd("autohilite", "true"),
    pd("showname", "true"),
    pd("textfont", ""),
    pd("textsize", "12"),
];
const FIELD_PROPS: &[PropDef] = &[
    pd("rect", "0,0,200,100"),
    pd("visible", "true"),
    pd("enabled", "true"),
    pd("text", ""),
    pd("locktext", "false"),
    pd("style", "standard"),
    pd("textfont", ""),
    pd("textsize", "12"),
    pd("textalign", "left"),
    pd("dontwrap", "false"),
];
const SCROLLBAR_PROPS: &[PropDef] = &[
    pd("rect", "0,0,16,100"),
    pd("visible", "true"),
    pd("enabled", "true"),
    pd("orientation", "vertical"),
    pd("startvalue", "0"),
    pd("endvalue", "100"),
    pd("thumbposition", "0"),
];
const IMAGE_PROPS: &[PropDef] = &[
    pd("rect", "0,0,100,100"),
    pd("visible", "true"),
    pd("filename", ""),
];
const GRAPHIC_PROPS: &[PropDef] = &[
    pd("rect", "0,0,100,100"),
    pd("visible", "true"),
    pd("style", "rectangle"),
    pd("linesize", "1"),
    pd("forecolor", ""),
    pd("backcolor", ""),
];

fn prototype_props(kind: ObjKind) -> &'static [PropDef] {
    match kind {
        ObjKind::Stack => STACK_PROPS,
        ObjKind::Background => BACKGROUND_PROPS,
        ObjKind::Card => CARD_PROPS,
        ObjKind::Group => GROUP_PROPS,
        ObjKind::Button => BUTTON_PROPS,
        ObjKind::Field => FIELD_PROPS,
        ObjKind::Scrollbar => SCROLLBAR_PROPS,
        ObjKind::Image => IMAGE_PROPS,
        ObjKind::Graphic => GRAPHIC_PROPS,
    }
}

/// Backgrounds and cards take their size from the stack, so they have no rect.
fn has_rect(kind: ObjKind) -> bool {
    !matches!(kind, ObjKind::Background | ObjKind::Card)
}

fn is_bool_prop(name: &str) -> bool {
    matches!(
        name,
        "visible"
            | "enabled"
            | "resizable"
            | "cantdelete"
            | "marked"
            | "hilite"
            | "autohilite"
            | "showname"
            | "locktext"
            | "dontwrap"
    )
}

// ----- rectangles -----------------------------------------------------------

fn parse_rect(text: &str) -> Option<[f64; 4]> {
    let parts: Vec<f64> = text
        .split(',')
        .map(|p| p.trim().parse::<f64>())
        .collect::<Result<Vec<f64>, _>>()
        .ok()?;
    if parts.len() == 4 && parts.iter().all(|n| n.is_finite()) {
        Some([parts[0], parts[1], parts[2], parts[3]])
    } else {
        None
    }
}

fn format_rect(r: [f64; 4]) -> String {
    format!(
        "{},{},{},{}",
        format_number(r[0]),
        format_number(r[1]),
        format_number(r[2]),
        format_number(r[3])
    )
}

/// A computed property of a rectangle, such as its width.
fn rect_part(rect: &str, key: &str) -> Option<String> {
    let [l, t, r, b] = parse_rect(rect)?;
    match key {
        "left" => Some(format_number(l)),
        "top" => Some(format_number(t)),
        "right" => Some(format_number(r)),
        "bottom" => Some(format_number(b)),
        "width" => Some(format_number(r - l)),
        "height" => Some(format_number(b - t)),
        "loc" | "location" => Some(format!(
            "{},{}",
            format_number((l + r) / 2.0),
            format_number((t + b) / 2.0)
        )),
        _ => None,
    }
}

/// The rectangle after setting one of its parts. Moving a side moves the
/// whole rectangle. Setting the width or height keeps the top left corner.
fn changed_rect(key: &str, [l, t, r, b]: [f64; 4], value: &str) -> Result<[f64; 4], XtError> {
    match key {
        "rect" => parse_rect(value)
            .ok_or_else(|| XtError::new("Expected four numbers, like 10,20,110,60")),
        "loc" | "location" => {
            let parts: Vec<&str> = value.split(',').collect();
            if parts.len() != 2 {
                return Err(XtError::new("Expected two numbers, like 100,80"));
            }
            let x = to_number(parts[0])?;
            let y = to_number(parts[1])?;
            let (dx, dy) = (x - (l + r) / 2.0, y - (t + b) / 2.0);
            Ok([l + dx, t + dy, r + dx, b + dy])
        }
        _ => {
            let n = to_number(value)?;
            Ok(match key {
                "left" => [n, t, r + (n - l), b],
                "top" => [l, n, r, b + (n - t)],
                "right" => [l + (n - r), t, n, b],
                "bottom" => [l, t + (n - b), r, n],
                "width" => [l, t, l + n, b],
                _ => [l, t, r, t + n], // height
            })
        }
    }
}

fn gone() -> XtError {
    XtError::new("That object no longer exists")
}

// ----- the world ------------------------------------------------------------

impl World {
    pub fn get(&self, id: ObjId) -> Option<&Object> {
        self.slots
            .get(id.index)
            .filter(|slot| slot.gen == id.gen)
            .and_then(|slot| slot.obj.as_ref())
    }

    pub fn get_mut(&mut self, id: ObjId) -> Option<&mut Object> {
        self.slots
            .get_mut(id.index)
            .filter(|slot| slot.gen == id.gen)
            .and_then(|slot| slot.obj.as_mut())
    }

    fn insert(&mut self, object: Object) -> ObjId {
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index];
            slot.obj = Some(object);
            ObjId {
                index,
                gen: slot.gen,
            }
        } else {
            self.slots.push(Slot {
                gen: 0,
                obj: Some(object),
            });
            ObjId {
                index: self.slots.len() - 1,
                gen: 0,
            }
        }
    }

    /// Make an object of `kind` with its prototype's default properties and
    /// add it to the end of its parent's children.
    pub fn create(&mut self, kind: ObjKind, name: &str, parent: Option<ObjId>) -> ObjId {
        self.next_id += 1;
        let id = self.next_id;
        let mut props = HashMap::new();
        props.insert("name".to_string(), name.to_string());
        props.insert("script".to_string(), String::new());
        for def in prototype_props(kind) {
            props.insert(def.name.to_string(), def.default.to_string());
        }
        let obj_id = self.insert(Object {
            kind,
            id,
            parent,
            children: Vec::new(),
            background: None,
            props,
            handlers: HashMap::new(),
        });
        self.by_id.insert(id, obj_id);
        if let Some(parent) = parent {
            if let Some(p) = self.get_mut(parent) {
                p.children.push(obj_id);
            }
        }
        obj_id
    }

    /// Make a stack with one background and one card.
    pub fn create_stack(&mut self, name: &str) -> ObjId {
        let stack = self.create(ObjKind::Stack, name, None);
        self.stacks.push(stack);
        let background = self.create(ObjKind::Background, "", Some(stack));
        let card = self.create(ObjKind::Card, "", Some(stack));
        if let Some(c) = self.get_mut(card) {
            c.background = Some(background);
        }
        stack
    }

    /// Delete an object and everything inside it.
    pub fn delete(&mut self, id: ObjId) {
        let Some(obj) = self.get(id) else { return };
        let children = obj.children.clone();
        let parent = obj.parent;
        let object_id = obj.id;
        for child in children {
            self.delete(child);
        }
        if let Some(parent) = parent {
            if let Some(p) = self.get_mut(parent) {
                p.children.retain(|c| *c != id);
            }
        }
        self.stacks.retain(|s| *s != id);
        self.by_id.remove(&object_id);
        let slot = &mut self.slots[id.index];
        slot.obj = None;
        slot.gen += 1;
        self.free.push(id.index);
    }

    /// The text that stands for an object: `button id 12`.
    pub fn ref_string(&self, id: ObjId) -> Option<String> {
        self.get(id)
            .map(|o| format!("{} id {}", kind_name(o.kind), o.id))
    }

    /// Find the object a reference such as `button id 12` stands for.
    pub fn lookup_ref(&self, text: &str) -> Result<ObjId, XtError> {
        let words: Vec<&str> = text.split_whitespace().collect();
        if words.len() >= 3 && words[1].eq_ignore_ascii_case("id") {
            if let (Some((kind, false)), Ok(n)) = (kind_of_word(words[0]), words[2].parse::<i64>())
            {
                if let Some(&obj_id) = self.by_id.get(&n) {
                    if self.get(obj_id).is_some_and(|o| o.kind == kind) {
                        return Ok(obj_id);
                    }
                }
                return Err(XtError::new(format!(
                    "{} no longer exists",
                    words[..3].join(" ")
                )));
            }
        }
        Err(XtError::new(format!(
            "Expected an object but found \"{text}\""
        )))
    }

    pub fn by_numeric_id(&self, id: i64) -> Option<ObjId> {
        self.by_id.get(&id).copied()
    }

    pub fn children_of_kind(&self, parent: ObjId, kind: ObjKind) -> Vec<ObjId> {
        match self.get(parent) {
            Some(p) => p
                .children
                .iter()
                .copied()
                .filter(|c| self.get(*c).is_some_and(|o| o.kind == kind))
                .collect(),
            None => Vec::new(),
        }
    }

    /// The first object in `list` with this name, ignoring case.
    pub fn first_named(&self, list: &[ObjId], name: &str) -> Option<ObjId> {
        list.iter().copied().find(|id| {
            self.get(*id).is_some_and(|o| {
                o.props
                    .get("name")
                    .is_some_and(|n| n.eq_ignore_ascii_case(name))
            })
        })
    }

    /// Position among objects of the same kind in the same container, from 1.
    pub fn number_of(&self, id: ObjId) -> usize {
        let Some(obj) = self.get(id) else { return 0 };
        let siblings = match obj.parent {
            Some(parent) => self.children_of_kind(parent, obj.kind),
            None => self.stacks.clone(),
        };
        siblings.iter().position(|s| *s == id).map_or(0, |i| i + 1)
    }

    /// The stack an object belongs to.
    pub fn stack_of(&self, id: ObjId) -> Option<ObjId> {
        let mut current = Some(id);
        while let Some(c) = current {
            let obj = self.get(c)?;
            if obj.kind == ObjKind::Stack {
                return Some(c);
            }
            current = obj.parent;
        }
        None
    }

    /// Where a message goes, in order: the object, its groups, its card, the
    /// card's background, and the stack.
    pub fn path(&self, start: ObjId) -> Vec<ObjId> {
        let mut out = Vec::new();
        let mut current = Some(start);
        while let Some(id) = current {
            let Some(obj) = self.get(id) else { break };
            out.push(id);
            if obj.kind == ObjKind::Card {
                if let Some(background) = obj.background {
                    out.push(background);
                }
            }
            current = obj.parent;
        }
        out
    }

    pub fn first_card(&self, stack: ObjId) -> Option<ObjId> {
        self.children_of_kind(stack, ObjKind::Card).first().copied()
    }

    // ----- properties --------------------------------------------------------

    pub fn get_prop(&self, id: ObjId, name: &str) -> Result<String, XtError> {
        let obj = self.get(id).ok_or_else(gone)?;
        let key = name.to_lowercase();
        match key.as_str() {
            "id" => return Ok(obj.id.to_string()),
            "number" => return Ok(self.number_of(id).to_string()),
            "owner" => {
                return Ok(obj
                    .parent
                    .and_then(|p| self.ref_string(p))
                    .unwrap_or_default())
            }
            _ => {}
        }
        if has_rect(obj.kind) {
            if let Some(rect) = obj.props.get("rect") {
                if let Some(part) = rect_part(rect, &key) {
                    return Ok(part);
                }
            }
        }
        Ok(obj.props.get(&key).cloned().unwrap_or_default())
    }

    pub fn set_prop(&mut self, id: ObjId, name: &str, value: &str) -> Result<(), XtError> {
        let key = name.to_lowercase();
        let kind = self.get(id).ok_or_else(gone)?.kind;
        if matches!(key.as_str(), "id" | "number" | "owner") {
            return Err(XtError::new(format!(
                "The {key} of an object can't be changed"
            )));
        }
        let rect_key = matches!(
            key.as_str(),
            "rect" | "left" | "top" | "right" | "bottom" | "width" | "height" | "loc" | "location"
        );
        if rect_key && has_rect(kind) {
            let current = self
                .get(id)
                .and_then(|o| o.props.get("rect"))
                .and_then(|r| parse_rect(r))
                .unwrap_or([0.0; 4]);
            let updated = changed_rect(&key, current, value)?;
            if let Some(obj) = self.get_mut(id) {
                obj.props.insert("rect".to_string(), format_rect(updated));
            }
            return Ok(());
        }
        let stored = if is_bool_prop(&key) {
            match value.trim().to_lowercase().as_str() {
                "true" => "true".to_string(),
                "false" => "false".to_string(),
                _ => {
                    return Err(XtError::new(format!(
                        "Expected true or false but found \"{value}\""
                    )))
                }
            }
        } else {
            value.to_string()
        };
        if let Some(obj) = self.get_mut(id) {
            obj.props.insert(key, stored);
        }
        Ok(())
    }
}
