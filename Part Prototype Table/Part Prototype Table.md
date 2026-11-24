# Part Prototype Table v0

Object model for the xTalk-in-Rust project · November 22, 2026 · Status: draft that the headless code implements

## 1. Purpose and Status

This document defines the parts a script can create and talk to: stacks, backgrounds, cards, groups, buttons, fields, scrollbars, images, and graphics. It is the contract between the engine and any toolkit backend. The engine owns the objects, their properties, their scripts, and the message path. A backend only realizes each part on screen and reports events.

Rules have IDs (`OBJ-1`) and a status: **Decided**, **Provisional**, or **Open**, as in the Dialect Spec. Where a property is listed here, the headless engine implements it today. Items marked *planned* are not built yet.

## 2. The Object Tree

| Kind | Parent | Children | Notes |
| --- | --- | --- | --- |
| stack | none | backgrounds, cards | Top level. Shown in a window by a backend. |
| background | stack | groups, controls | Shared by the cards that use it. |
| card | stack | groups, controls | Uses exactly one background. |
| group | card, background, or group | controls, groups | A container for other controls. |
| button | card, background, or group | none |  |
| field | card, background, or group | none | A container: its text is its value. |
| scrollbar | card, background, or group | none |  |
| image | card, background, or group | none |  |
| graphic | card, background, or group | none |  |

- Every object has a unique numeric **id**, assigned when it is made and never reused.
- An object's **number** is its position among the parts of the same kind in its container, counting from 1 in creation order. Creation order is also front-to-back order.
- A **name** is optional text. Names need not be unique. A name lookup takes the first match and ignores case.
- The engine starts with one stack named Untitled, holding one background and one card.

## 3. The Message Path

A message sent to an object travels through its path until a handler is found:

| Object | Path |
| --- | --- |
| control on a card | control, its groups, card, the card's background, stack, engine |
| control on a background | control, its groups, background, stack, engine |
| card | card, its background, stack, engine |
| background | background, stack, engine |
| stack | stack, engine |

"Engine" means handlers defined in loose scripts with `on name`, which act as a shared library.

## 4. Properties of Every Part

| Property | Type | Default | Notes |
| --- | --- | --- | --- |
| `name` | text | empty |  |
| `id` | whole number | assigned | Read-only. |
| `number` | whole number | computed | Read-only. |
| `owner` | reference | computed | Read-only. The parent, or empty for a stack. |
| `script` | text | empty | Handlers only. Compiled when set. |

Parts that have a `rect` also have these, all computed from it:

| Property | Meaning |
| --- | --- |
| `rect` | `left,top,right,bottom` |
| `left`, `top`, `right`, `bottom` | Setting one moves the part so that edge is there. |
| `width`, `height` | Setting one resizes the part and keeps its top left corner. |
| `loc` | The center, as `x,y`. Setting it moves the part. |

Any other name is a **custom property**. Reading one that was never set gives empty text. Setting one creates it.

## 5. Properties by Kind

Built-in properties named `visible`, `enabled`, `resizable`, `cantDelete`, `marked`, `hilite`, `autoHilite`, `showName`, `lockText`, and `dontWrap` accept only `true` or `false`.

### stack

| Property | Type | Default |
| --- | --- | --- |
| `rect` | rectangle | 100,100,612,442 |
| `visible` | boolean | true |
| `resizable` | boolean | true |
| `cantDelete` | boolean | false |

### background

| Property | Type | Default |
| --- | --- | --- |
| `cantDelete` | boolean | false |

### card

| Property | Type | Default |
| --- | --- | --- |
| `cantDelete` | boolean | false |
| `marked` | boolean | false |

### group

| Property | Type | Default |
| --- | --- | --- |
| `rect` | rectangle | 0,0,100,100 |
| `visible` | boolean | true |

### button

| Property | Type | Default | Notes |
| --- | --- | --- | --- |
| `rect` | rectangle | 0,0,100,24 |  |
| `visible` | boolean | true |  |
| `enabled` | boolean | true |  |
| `label` | text | empty |  |
| `style` | text | standard | Also checkbox, radiobutton, popup, and others. |
| `hilite` | boolean | false | Pressed or checked. |
| `autoHilite` | boolean | true |  |
| `showName` | boolean | true |  |
| `textFont` | text | empty | Empty means the toolkit's default. |
| `textSize` | number | 12 |  |

### field

| Property | Type | Default | Notes |
| --- | --- | --- | --- |
| `rect` | rectangle | 0,0,200,100 |  |
| `visible` | boolean | true |  |
| `enabled` | boolean | true |  |
| `text` | text | empty | The field's value. |
| `lockText` | boolean | false | True makes it read-only for the person. |
| `style` | text | standard | Also scrolling and others. |
| `textFont` | text | empty |  |
| `textSize` | number | 12 |  |
| `textAlign` | text | left |  |
| `dontWrap` | boolean | false |  |

### scrollbar

| Property | Type | Default |
| --- | --- | --- |
| `rect` | rectangle | 0,0,16,100 |
| `visible` | boolean | true |
| `enabled` | boolean | true |
| `orientation` | text | vertical |
| `startValue` | number | 0 |
| `endValue` | number | 100 |
| `thumbPosition` | number | 0 |

### image

| Property | Type | Default |
| --- | --- | --- |
| `rect` | rectangle | 0,0,100,100 |
| `visible` | boolean | true |
| `filename` | text | empty |

### graphic

| Property | Type | Default |
| --- | --- | --- |
| `rect` | rectangle | 0,0,100,100 |
| `visible` | boolean | true |
| `style` | text | rectangle |
| `lineSize` | number | 1 |
| `foreColor` | color | empty |
| `backColor` | color | empty |

## 6. Presentation Roles

Each kind declares a role. A backend registers a widget for each role it can draw natively. Any role a backend lacks falls back to a themed, drawn widget, so every role works everywhere.

| Kind (and style) | Role | Native example: macOS | Native example: Windows | Native example: GTK |
| --- | --- | --- | --- | --- |
| stack | Window | NSWindow | HWND window | GtkWindow |
| card | Viewport | content NSView | child window | GtkFixed |
| background | none | none | none | none |
| group | Container | NSView | child window | GtkFixed |
| button (standard) | PushButton | NSButton | BUTTON control | GtkButton |
| button (checkbox) | Checkbox | NSButton, switch | BUTTON, checkbox | GtkCheckButton |
| button (radiobutton) | RadioButton | NSButton, radio | BUTTON, radio | GtkCheckButton in a group |
| button (popup) | PopupMenu | NSPopUpButton | COMBOBOX | GtkDropDown |
| field (standard) | TextField | NSTextField | EDIT control | GtkEntry |
| field (scrolling) | TextArea | NSTextView in a scroll view | multiline EDIT | GtkTextView |
| scrollbar | ScrollBar | NSScroller | SCROLLBAR | GtkScrollbar |
| image | Image | NSImageView | STATIC image | GtkPicture |
| graphic | Shape | drawn | drawn | drawn |

A backend answers two questions for the engine: how big is this text (for properties that depend on measuring), and which part was hit. Everything else flows through property changes and events.

## 7. System Messages

| Message | Sent to | When | Sent today |
| --- | --- | --- | --- |
| `openCard` | the card | After `go` arrives at it | yes |
| `closeCard` | the card | Before `go` leaves it | yes |
| `openStack`, `closeStack` | the stack | A stack is shown or closed | planned |
| `mouseDown`, `mouseUp` | the control under the pointer | Pointer events from a backend | planned |
| `mouseEnter`, `mouseLeave` | the control | The pointer crosses its edge | planned |
| `keyDown`, `keyUp` | the focused field | Key events from a backend | planned |
| `idle` | the current card | The host is idle | planned |

A system message with no handler is ignored. Any other message with no handler is an error.

## 8. References

| Form | Examples |
| --- | --- |
| By number | `card 3`, `button 1`, `field 2 of card 4` |
| By name | `button "OK"`, `card "Intro"`, `stack "Main"` |
| By id | `card id 12`, `button id 40` |
| By position | `first card`, `last button`, `next card`, `previous card`, `any card` |
| Current | `this card`, `this stack`, `this background` |
| In a script | `me` (the object whose script is running), `the target` (the object the message was first sent to) |
| From a variable | `the name of tObject` |
| Inside another | `button 1 of card 2 of stack "Main"` |

Words for kinds accept the short forms `cd`, `btn`, `fld`, `bkgnd`, and `bg`. A name or number that is not a literal needs parentheses: `button (tName)`.

A reference as a value is the text `kind id N`, such as `button id 40`. It can be stored in a variable and used again.

## 9. Commands and Forms

| Form | Meaning |
| --- | --- |
| `create button "OK" [in card 2]` | Make a part. `it` holds its reference. |
| `create card`, `create stack "Name"` | New cards and stacks become current. |
| `delete button 1` | Delete a part and its children. |
| `go [to] card 2`, `go next`, `go previous` | Change the current card. |
| `send mouseUp to button 1` | Send a message and wait for the handler. |
| `pass mouseUp` | Send the message on and end this handler. |
| `show button 1`, `hide button 1` | Set `visible`. |
| `set the label of button 1 to "Go"` | Set a property. |
| `put the label of button 1 into x` | Read a property. |
| `put "hi" into field 1`, `put field 1 into x` | A field is a container. |
| `the number of cards`, `the number of buttons of card 2` | Count parts. |

## 10. Rules

| ID | Rule | Status |
| --- | --- | --- |
| OBJ-1 | Every part has a unique id. References name an object by kind and id and keep working after other parts are deleted. | Decided |
| OBJ-2 | The default container for a `card` or `background` is the current stack. For the other parts it is the current card. `stack` is global. | Provisional |
| OBJ-3 | Reading a property that is not set gives empty text. Setting an unknown property creates a custom property. | Provisional |
| OBJ-4 | `id`, `number`, and `owner` are read-only. | Decided |
| OBJ-5 | A field is a container: it can be read, put into, and chunked. Other parts are not containers. | Decided |
| OBJ-6 | A reference to a field in a value position gives its text. In an object position (after `of`, `to`, or `me`) it always gives the reference. | Provisional |
| OBJ-7 | An object script contains handlers only. Setting `script` compiles it at once, and an error is reported by the `set`. | Decided |
| OBJ-8 | A message sent from a running handler starts at `me`. A message sent from loose statements starts at the current card. | Provisional |
| OBJ-9 | After the path, engine-level handlers are tried, then built-in functions. | Decided |
| OBJ-10 | `send` waits for the handler. An unhandled system message is ignored. Any other unhandled message is an error. | Decided |
| OBJ-11 | `pass` ends the current handler and sends the message to the objects after the one whose script holds the handler. | Decided |
| OBJ-12 | `go` sends `closeCard` to the old card, then `openCard` to the new one. | Provisional |
| OBJ-13 | The last card of a stack, the last stack, and a background that cards use cannot be deleted. | Provisional |
| OBJ-14 | Deleting a part deletes its children. A reference to a deleted part is an error. | Decided |
| OBJ-15 | Setting `left`, `top`, `right`, or `bottom` moves a part. Setting `width` or `height` resizes it and keeps its top left corner. | Provisional |

## 11. Planned and Open

- **Timers:** `send ... in n seconds`, and cancelling them.
- **Arguments on `send`**, and `dispatch`.
- **Groups:** grouping existing parts, and `layer` or `bring closer` ordering.
- **Substacks and stack files.**
- **Backend events:** pointer and key messages, focus, and menus.
- **Colors:** the accepted forms (names, `r,g,b`, hex) and theme defaults.
- **Open:** whether reading an unknown built-in-looking property should be an error, to catch typos.
