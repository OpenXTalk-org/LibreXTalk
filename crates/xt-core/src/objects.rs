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
    /// Objects other than stacks, by their stack and their id. Ids are
    /// numbered within each stack (OBJ-15), as in HyperCard and LiveCode, so
    /// an object keeps its id when its stack is saved and opened again.
    by_id: HashMap<(ObjId, i64), ObjId>,
    /// The last id used in each stack.
    next_ids: HashMap<ObjId, i64>,
    /// Stacks by their id. A stack's own id is unique in the whole engine,
    /// so references can name the stack (OBJ-16).
    stack_ids: HashMap<i64, ObjId>,
    next_stack_id: i64,
    pub stacks: Vec<ObjId>,
    pub current_stack: Option<ObjId>,
    pub current_card: Option<ObjId>,
    /// The file each stack was opened from or last saved to. This is kept
    /// out of the stack's properties so it is never written into the file
    /// itself, and moving a file does not change its contents (SFM-6).
    pub files: HashMap<ObjId, String>,
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
        ObjKind::Part => "part",
        ObjKind::AudioClip => "audioClip",
        ObjKind::VideoClip => "videoClip",
    }
}

/// Is this kind a control: something that sits on a card, a background, or
/// in a group? These are the kinds the generic `part` matches (OBJ-13).
pub fn is_control(kind: ObjKind) -> bool {
    matches!(
        kind,
        ObjKind::Group
            | ObjKind::Button
            | ObjKind::Field
            | ObjKind::Scrollbar
            | ObjKind::Image
            | ObjKind::Graphic
            | ObjKind::Part
    )
}

/// Resource objects belong to a stack and are not drawn (OBJ-14).
pub fn is_resource(kind: ObjKind) -> bool {
    matches!(kind, ObjKind::AudioClip | ObjKind::VideoClip)
}

/// Does an object of kind `actual` answer to a reference of kind `wanted`?
/// A reference to `part 2` finds a button as well as a plain part.
pub fn kind_matches(wanted: ObjKind, actual: ObjKind) -> bool {
    wanted == actual || (wanted == ObjKind::Part && is_control(actual))
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
        // `widget` and `control` are synonyms for `part`, as in OpenXTalk,
        // which kept `control` for HyperCard compatibility (OBJ-13).
        "part" | "widget" | "control" => (ObjKind::Part, false),
        "parts" | "widgets" | "controls" => (ObjKind::Part, true),
        "audioclip" => (ObjKind::AudioClip, false),
        "audioclips" => (ObjKind::AudioClip, true),
        "videoclip" => (ObjKind::VideoClip, false),
        "videoclips" => (ObjKind::VideoClip, true),
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

const PART_PROPS: &[PropDef] = &[pd("rect", "0,0,100,100"), pd("visible", "true")];
// Clip data (the samples or the movie bytes) arrives with stack files, which
// need binary values. Until then a clip holds only its settings.
const AUDIOCLIP_PROPS: &[PropDef] = &[pd("loudness", "100")];
const VIDEOCLIP_PROPS: &[PropDef] = &[pd("framerate", "0"), pd("scale", "1")];

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
        ObjKind::Part => PART_PROPS,
        ObjKind::AudioClip => AUDIOCLIP_PROPS,
        ObjKind::VideoClip => VIDEOCLIP_PROPS,
    }
}

/// Backgrounds and cards take their size from the stack, and resource
/// objects are not drawn, so none of them has a rect.
fn has_rect(kind: ObjKind) -> bool {
    !matches!(kind, ObjKind::Background | ObjKind::Card) && !is_resource(kind)
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

// ----- stacks as plain trees ------------------------------------------------

/// One object of a stack, with everything a stack file needs to keep, as
/// plain data (SFM-1). This is the bridge between the object model and the
/// stack file formats: the engine turns a stack into a tree when saving and
/// a tree into a stack when opening, and each file format reads and writes
/// trees. The engine never sees file bytes, and the formats never see the
/// engine's arena.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PartNode {
    /// The kind's name, as scripts write it: `stack`, `button`, `audioClip`.
    pub kind: String,
    pub id: i64,
    /// Properties that differ from the kind's defaults, sorted by name, with
    /// lowercase names. The name is here when it is not empty. The script is
    /// kept apart, and computed properties (`width`, `number`) are left out.
    pub props: Vec<(String, String)>,
    pub script: String,
    /// For a card: the id of the background it uses.
    pub background: Option<i64>,
    pub children: Vec<PartNode>,
}

/// Can an object of kind `child` live directly inside one of kind `parent`?
fn may_contain(parent: ObjKind, child: ObjKind) -> bool {
    match parent {
        ObjKind::Stack => {
            matches!(child, ObjKind::Background | ObjKind::Card) || is_resource(child)
        }
        ObjKind::Card | ObjKind::Background | ObjKind::Group => is_control(child),
        _ => false,
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
        self.create_with_id(kind, name, parent, None)
    }

    /// Like `create`, but with a chosen id (when opening a stack file). The
    /// caller makes sure the id is free in its stack.
    fn create_with_id(
        &mut self,
        kind: ObjKind,
        name: &str,
        parent: Option<ObjId>,
        wanted: Option<i64>,
    ) -> ObjId {
        // A stack takes the next engine-wide stack id. Anything else takes
        // the next id of its stack. Every object but a stack has a parent,
        // so it always has a stack.
        let stack = if kind == ObjKind::Stack {
            None
        } else {
            parent.and_then(|p| self.stack_of(p))
        };
        let id = match stack {
            None => {
                let id = wanted.unwrap_or(self.next_stack_id + 1);
                self.next_stack_id = self.next_stack_id.max(id);
                id
            }
            Some(stack) => {
                let last = self.next_ids.entry(stack).or_insert(0);
                let id = wanted.unwrap_or(*last + 1);
                *last = (*last).max(id);
                id
            }
        };
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
        match stack {
            None => {
                self.stack_ids.insert(id, obj_id);
            }
            Some(stack) => {
                self.by_id.insert((stack, id), obj_id);
            }
        }
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
        let is_stack = obj.kind == ObjKind::Stack;
        let stack = self.stack_of(id);
        for child in children {
            self.delete(child);
        }
        if let Some(parent) = parent {
            if let Some(p) = self.get_mut(parent) {
                p.children.retain(|c| *c != id);
            }
        }
        self.stacks.retain(|s| *s != id);
        self.files.remove(&id);
        if is_stack {
            self.stack_ids.remove(&object_id);
            self.next_ids.remove(&id);
        } else if let Some(stack) = stack {
            self.by_id.remove(&(stack, object_id));
        }
        let slot = &mut self.slots[id.index];
        slot.obj = None;
        slot.gen += 1;
        self.free.push(id.index);
    }

    /// The text that stands for an object: `stack id 1`, or
    /// `button id 12 of stack id 1`, because ids are numbered per stack
    /// (OBJ-16).
    pub fn ref_string(&self, id: ObjId) -> Option<String> {
        let obj = self.get(id)?;
        let kind = kind_name(obj.kind);
        if obj.kind == ObjKind::Stack {
            return Some(format!("{kind} id {}", obj.id));
        }
        let stack = self.get(self.stack_of(id)?)?;
        Some(format!("{kind} id {} of stack id {}", obj.id, stack.id))
    }

    /// Find the object a reference stands for: `button id 12 of stack id 1`,
    /// or the short `button id 12`, which means the current stack.
    pub fn lookup_ref(&self, text: &str) -> Result<ObjId, XtError> {
        let words: Vec<&str> = text.split_whitespace().collect();
        let is_id = |w: &str| w.eq_ignore_ascii_case("id");
        let shape_ok = match words.len() {
            3 => is_id(words[1]),
            7 => {
                is_id(words[1])
                    && words[3].eq_ignore_ascii_case("of")
                    && kind_of_word(words[4]) == Some((ObjKind::Stack, false))
                    && is_id(words[5])
            }
            _ => false,
        };
        if shape_ok {
            if let (Some((kind, false)), Ok(n)) = (kind_of_word(words[0]), words[2].parse::<i64>())
            {
                let stack = if words.len() == 7 {
                    words[6]
                        .parse::<i64>()
                        .ok()
                        .and_then(|m| self.stack_ids.get(&m).copied())
                } else {
                    self.current_stack
                };
                return self
                    .find_by_id(kind, n, stack)
                    .ok_or_else(|| XtError::new(format!("{text} no longer exists")));
            }
        }
        Err(XtError::new(format!(
            "Expected an object but found \"{text}\""
        )))
    }

    /// The object of `kind` with id `n`: a stack by its engine-wide id, or
    /// anything else by its id in `stack`.
    pub fn find_by_id(&self, kind: ObjKind, n: i64, stack: Option<ObjId>) -> Option<ObjId> {
        let found = if kind == ObjKind::Stack {
            self.stack_ids.get(&n).copied()
        } else {
            self.by_id.get(&(stack?, n)).copied()
        };
        found.filter(|id| self.get(*id).is_some_and(|o| kind_matches(kind, o.kind)))
    }

    pub fn children_of_kind(&self, parent: ObjId, kind: ObjKind) -> Vec<ObjId> {
        match self.get(parent) {
            Some(p) => p
                .children
                .iter()
                .copied()
                .filter(|c| self.get(*c).is_some_and(|o| kind_matches(kind, o.kind)))
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

    // ----- stack trees -------------------------------------------------------

    /// The stack (or any object) as a tree of plain data, for saving.
    pub fn export(&self, id: ObjId) -> Result<PartNode, XtError> {
        let obj = self.get(id).ok_or_else(gone)?;
        let defaults = prototype_props(obj.kind);
        let mut props: Vec<(String, String)> = obj
            .props
            .iter()
            .filter(|(k, v)| {
                if k.as_str() == "script" {
                    return false;
                }
                let default = if k.as_str() == "name" {
                    Some("")
                } else {
                    defaults
                        .iter()
                        .find(|d| d.name == k.as_str())
                        .map(|d| d.default)
                };
                default != Some(v.as_str())
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        props.sort();
        let background = obj.background.and_then(|b| self.get(b)).map(|b| b.id);
        let mut children = Vec::with_capacity(obj.children.len());
        for &child in &obj.children {
            children.push(self.export(child)?);
        }
        Ok(PartNode {
            kind: kind_name(obj.kind).to_string(),
            id: obj.id,
            props,
            script: obj.props.get("script").cloned().unwrap_or_default(),
            background,
            children,
        })
    }

    /// Build a stack from a tree, for opening (SFM-2). Returns the new stack
    /// and every object that has a script, so the caller can compile them.
    ///
    /// Ids are kept when none of them is in use already. Ids are unique in
    /// the whole engine for now, not per stack, so when any id clashes every
    /// object of the opened stack gets a fresh id instead (SFM-3).
    pub fn import_stack(
        &mut self,
        tree: &PartNode,
    ) -> Result<(ObjId, Vec<(ObjId, String)>), XtError> {
        if kind_of_word(&tree.kind) != Some((ObjKind::Stack, false)) {
            return Err(XtError::new(format!(
                "Expected a stack at the top of the file but found a {}",
                tree.kind
            )));
        }
        // Ids inside the stack are its own, so they are always kept. Only
        // the stack's id must be unique in the engine: if another open stack
        // has it, the opened stack gets a fresh one (SFM-3).
        let mut ids = Vec::new();
        for child in &tree.children {
            collect_ids(child, &mut ids);
        }
        let mut seen = std::collections::HashSet::new();
        for &id in &ids {
            if id < 1 || !seen.insert(id) {
                return Err(XtError::new(format!(
                    "The stack file uses the object id {id} more than once, or an id below 1"
                )));
            }
        }
        let stack_id = (tree.id >= 1 && !self.stack_ids.contains_key(&tree.id)).then_some(tree.id);
        let stack = self.create_with_id(ObjKind::Stack, "", None, stack_id);
        let mut new_of_old = HashMap::new();
        let mut scripts = Vec::new();
        let built = self.set_imported_props(stack, tree).and_then(|()| {
            if !tree.script.is_empty() {
                scripts.push((stack, tree.script.clone()));
            }
            for child in &tree.children {
                self.import_node(child, stack, &mut new_of_old, &mut scripts)?;
            }
            Ok(())
        });
        if let Err(e) = built {
            // Remove whatever was made before the problem was found.
            self.delete(stack);
            return Err(e);
        }
        // Cards name their background by id, so link them once all exist.
        let mut cards = Vec::new();
        for child in &tree.children {
            collect_cards(child, &mut cards);
        }
        for (card_id, background) in cards {
            let card = new_of_old[&card_id];
            let link = match background {
                Some(b) => match new_of_old.get(&b) {
                    Some(&bg) if self.get(bg).is_some_and(|o| o.kind == ObjKind::Background) => {
                        Some(bg)
                    }
                    _ => {
                        self.delete(stack);
                        return Err(XtError::new(format!(
                            "Card id {card_id} uses background id {b}, which is not in the file"
                        )));
                    }
                },
                None => None,
            };
            if let Some(c) = self.get_mut(card) {
                c.background = link;
            }
        }
        if self.first_card(stack).is_none() {
            self.delete(stack);
            return Err(XtError::new("The stack in the file has no cards"));
        }
        self.stacks.push(stack);
        Ok((stack, scripts))
    }

    fn set_imported_props(&mut self, obj: ObjId, node: &PartNode) -> Result<(), XtError> {
        for (name, value) in &node.props {
            if let Err(e) = self.set_prop(obj, name, value) {
                return Err(XtError::new(format!(
                    "The {name} of {} id {} in the file is wrong: {}",
                    node.kind, node.id, e.message
                )));
            }
        }
        Ok(())
    }

    fn import_node(
        &mut self,
        node: &PartNode,
        parent: ObjId,
        new_of_old: &mut HashMap<i64, ObjId>,
        scripts: &mut Vec<(ObjId, String)>,
    ) -> Result<(), XtError> {
        let kind = match kind_of_word(&node.kind) {
            Some((kind, false)) => kind,
            _ => {
                return Err(XtError::new(format!(
                    "Unknown kind of object: {}",
                    node.kind
                )))
            }
        };
        if let Some(p) = self.get(parent) {
            if !may_contain(p.kind, kind) {
                let message = format!(
                    "{} id {} can't hold a {} (id {})",
                    kind_name(p.kind),
                    p.id,
                    node.kind,
                    node.id
                );
                return Err(XtError::new(message));
            }
        }
        let created = self.create_with_id(kind, "", Some(parent), Some(node.id));
        new_of_old.insert(node.id, created);
        self.set_imported_props(created, node)?;
        if !node.script.is_empty() {
            scripts.push((created, node.script.clone()));
        }
        for child in &node.children {
            self.import_node(child, created, new_of_old, scripts)?;
        }
        Ok(())
    }

    // ----- properties --------------------------------------------------------

    pub fn get_prop(&self, id: ObjId, name: &str) -> Result<String, XtError> {
        let obj = self.get(id).ok_or_else(gone)?;
        let key = name.to_lowercase();
        match key.as_str() {
            "id" => return Ok(obj.id.to_string()),
            "number" => return Ok(self.number_of(id).to_string()),
            "filename" if obj.kind == ObjKind::Stack => {
                return Ok(self.files.get(&id).cloned().unwrap_or_default())
            }
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
        if key == "filename" && kind == ObjKind::Stack {
            self.files.insert(id, value.to_string());
            return Ok(());
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

fn collect_ids(node: &PartNode, out: &mut Vec<i64>) {
    out.push(node.id);
    for child in &node.children {
        collect_ids(child, out);
    }
}

fn collect_cards(node: &PartNode, out: &mut Vec<(i64, Option<i64>)>) {
    if node.kind.eq_ignore_ascii_case("card") {
        out.push((node.id, node.background));
    }
    for child in &node.children {
        collect_cards(child, out);
    }
}
