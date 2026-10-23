//! Multi-line text area: soft-wrapped, grows from `min_lines` to
//! `max_lines` and then scrolls inside (the work item description editor
//! and comment composer, docs/design/work-item-detail.md §5.2, §7.3).
//!
//! Editing reuses [`crate::text_input`]'s model ([`EditState`]) and its
//! actions (bound in the `TextInput` key context, which the area also
//! declares, so `!TextInput` single-letter keys never fire while it has
//! focus). The area adds `↩` for a newline, `↑`/`↓` across visual rows,
//! and **atoms**: ranges painted as one token (mention chips) that the caret
//! steps over and `⌫` deletes whole ([`AreaState`]).
//!
//! The IME/text element follows [`crate::text_input`] (itself adapted from
//! GPUI's `examples/input.rs`, zui rev `18a89af`, Apache-2.0); wrapping uses
//! GPUI's `shape_text` and `WrappedLine` geometry.

use std::ops::Range;

use gpui::{
    App, AvailableSpace, Bounds, ClipboardItem, ContentMask, Context, CursorStyle, ElementId,
    ElementInputHandler, Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable,
    FontWeight, GlobalElementId, Hsla, KeyBinding, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, SharedString, Size, Style, TextRun,
    UTF16Selection, UnderlineStyle, Window, WrappedLine, actions, div, fill, point, prelude::*, px,
    relative, size,
};

use crate::text_input::{
    Backspace, Copy, Cut, Delete, DeleteToStart, EditState, End, Home, Left, Paste, Right,
    SelectAll, SelectLeft, SelectRight, ShowCharacterPalette,
};
use crate::theme::ActiveTheme;

actions!(
    text_area,
    [
        /// Insert a line break.
        Newline,
        /// Move the caret one visual row up.
        Up,
        /// Move the caret one visual row down.
        Down,
        /// Extend the selection one visual row up.
        SelectUp,
        /// Extend the selection one visual row down.
        SelectDown,
    ]
);

/// Key context of the area's own bindings (it also declares
/// [`crate::text_input::KEY_CONTEXT`]).
pub const KEY_CONTEXT: &str = "TextArea";

/// Register once at startup, next to [`crate::text_input::bindings`].
pub fn bindings() -> Vec<KeyBinding> {
    let cx = Some(KEY_CONTEXT);
    vec![
        KeyBinding::new("enter", Newline, cx),
        KeyBinding::new("shift-enter", Newline, cx),
        KeyBinding::new("up", Up, cx),
        KeyBinding::new("down", Down, cx),
        KeyBinding::new("shift-up", SelectUp, cx),
        KeyBinding::new("shift-down", SelectDown, cx),
    ]
}

/// Width of the painted caret, in px.
const CARET_WIDTH: f32 = 2.0;

/// A range painted and edited as one token (a mention chip), with the id
/// it stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Atom {
    pub range: Range<usize>,
    pub id: SharedString,
}

/// Text + selection + atoms. Edits that touch an atom remove it whole;
/// carets never rest inside one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AreaState {
    pub edit: EditState,
    atoms: Vec<Atom>,
}

impl AreaState {
    pub fn text(&self) -> &str {
        self.edit.text()
    }

    pub fn atoms(&self) -> &[Atom] {
        &self.atoms
    }

    /// Replace everything; `atoms` must be sorted, disjoint and on char
    /// boundaries of `text`.
    pub fn set(&mut self, text: &str, atoms: Vec<Atom>) {
        self.edit.set_text(text);
        self.atoms = atoms
            .into_iter()
            .filter(|a| a.range.end <= text.len() && a.range.start < a.range.end)
            .collect();
    }

    /// The text with every atom replaced by `token(id)` (what is sent).
    pub fn serialize(&self, token: impl Fn(&str) -> String) -> String {
        let text = self.text();
        let mut out = String::with_capacity(text.len());
        let mut at = 0;
        for atom in &self.atoms {
            out.push_str(&text[at..atom.range.start]);
            out.push_str(&token(&atom.id));
            at = atom.range.end;
        }
        out.push_str(&text[at..]);
        out
    }

    /// `offset` moved out of any atom it falls strictly inside: to the
    /// atom's end when moving forward, else its start.
    pub fn snap(&self, offset: usize, forward: bool) -> usize {
        match self
            .atoms
            .iter()
            .find(|a| a.range.start < offset && offset < a.range.end)
        {
            Some(atom) if forward => atom.range.end,
            Some(atom) => atom.range.start,
            None => offset,
        }
    }

    fn resolve(&self, range: Option<Range<usize>>) -> Range<usize> {
        let range = range
            .or_else(|| self.edit.marked())
            .unwrap_or_else(|| self.edit.selection());
        let end = range.end.min(self.text().len());
        range.start.min(end)..end
    }

    /// Replace `range` (default: the composition, else the selection) with
    /// `text`. A range touching an atom grows to cover it, and the atom
    /// goes. Returns whether the text changed.
    pub fn replace(&mut self, range: Option<Range<usize>>, text: &str) -> bool {
        let mut range = self.resolve(range);
        for atom in &self.atoms {
            let overlaps = atom.range.start < range.end && range.start < atom.range.end;
            let inside =
                range.is_empty() && atom.range.start < range.start && range.start < atom.range.end;
            if overlaps || inside {
                range.start = range.start.min(atom.range.start);
                range.end = range.end.max(atom.range.end);
            }
        }
        let removed = range.end - range.start;
        self.atoms
            .retain(|a| a.range.end <= range.start || a.range.start >= range.end);
        for atom in &mut self.atoms {
            if atom.range.start >= range.end {
                atom.range.start = atom.range.start - removed + text.len();
                atom.range.end = atom.range.end - removed + text.len();
            }
        }
        self.edit.replace(Some(range), text)
    }

    /// Replace `range` with `display` as one atom standing for `id`; the
    /// caret lands after it.
    pub fn insert_atom(&mut self, range: Range<usize>, display: &str, id: SharedString) {
        self.replace(Some(range.clone()), display);
        let start = range.start.min(self.text().len());
        let atom = Atom {
            range: start..start + display.len(),
            id,
        };
        let at = self
            .atoms
            .iter()
            .position(|a| a.range.start >= atom.range.end)
            .unwrap_or(self.atoms.len());
        self.atoms.insert(at, atom);
        self.edit.move_to(start + display.len());
    }

    /// Caret one grapheme (or one atom) left / right.
    pub fn step(&mut self, forward: bool, extend: bool) {
        let cursor = self.edit.cursor();
        let target = if forward {
            self.snap(self.edit.next_boundary(cursor), true)
        } else {
            self.snap(self.edit.previous_boundary(cursor), false)
        };
        if extend {
            self.edit.select_to(target);
        } else {
            let selection = self.edit.selection();
            let target = match (selection.is_empty(), forward) {
                (true, _) => target,
                (false, true) => selection.end,
                (false, false) => selection.start,
            };
            self.edit.move_to(target);
        }
    }

    /// `⌫`: the selection, else the grapheme or atom before the caret.
    pub fn backspace(&mut self) -> bool {
        if self.edit.selection().is_empty() {
            let cursor = self.edit.cursor();
            let previous = self.edit.previous_boundary(cursor);
            if previous == cursor {
                return false;
            }
            self.edit.select_to(previous);
        }
        self.replace(None, "")
    }

    /// `⌦`: the selection, else the grapheme or atom after the caret.
    pub fn delete(&mut self) -> bool {
        if self.edit.selection().is_empty() {
            let cursor = self.edit.cursor();
            let next = self.edit.next_boundary(cursor);
            if next == cursor {
                return false;
            }
            self.edit.select_to(next);
        }
        self.replace(None, "")
    }

    /// Start of the logical line holding `offset`.
    pub fn line_start(&self, offset: usize) -> usize {
        self.text()[..offset.min(self.text().len())]
            .rfind('\n')
            .map_or(0, |ix| ix + 1)
    }

    /// End of the logical line holding `offset`.
    pub fn line_end(&self, offset: usize) -> usize {
        let text = self.text();
        let offset = offset.min(text.len());
        text[offset..]
            .find('\n')
            .map_or(text.len(), |ix| offset + ix)
    }

    /// The text from the start of the caret's line up to the caret (the
    /// composer looks for an `@` query in it).
    pub fn before_caret_on_line(&self) -> (usize, &str) {
        let cursor = self.edit.cursor();
        let start = self.line_start(cursor);
        (start, &self.text()[start..cursor])
    }
}

/// Emitted after every change to the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAreaEvent {
    Changed,
}

/// Wrapped layout of the last paint, for mouse and vertical moves.
#[derive(Default)]
struct Measured {
    lines: Vec<WrappedLine>,
    /// Byte offset where each logical line starts.
    starts: Vec<usize>,
    /// Top of each logical line, relative to the content's top.
    tops: Vec<Pixels>,
    line_height: Pixels,
    bounds: Option<Bounds<Pixels>>,
}

impl Measured {
    /// Content-relative position of `offset` (top-left of its row).
    fn position(&self, offset: usize) -> Option<Point<Pixels>> {
        let ix = self
            .starts
            .iter()
            .rposition(|start| *start <= offset)?
            .min(self.lines.len().checked_sub(1)?);
        let local = offset - self.starts[ix];
        let p =
            self.lines[ix].position_for_index(local.min(self.lines[ix].len()), self.line_height)?;
        Some(point(p.x, p.y + self.tops[ix]))
    }

    /// Offset closest to a content-relative position.
    fn offset(&self, position: Point<Pixels>) -> usize {
        if self.lines.is_empty() {
            return 0;
        }
        let ix = self
            .tops
            .iter()
            .rposition(|top| *top <= position.y)
            .unwrap_or(0);
        let line = &self.lines[ix];
        let local = point(
            position.x.max(px(0.0)),
            (position.y - self.tops[ix]).max(px(0.0)),
        );
        let index = match line.closest_index_for_position(local, self.line_height) {
            Ok(index) | Err(index) => index,
        };
        self.starts[ix] + index.min(line.len())
    }
}

pub struct TextArea {
    focus_handle: FocusHandle,
    state: AreaState,
    placeholder: SharedString,
    accessible_label: SharedString,
    min_lines: usize,
    max_lines: usize,
    disabled: bool,
    measured: Measured,
    scroll_y: f32,
    /// Remembered x through vertical moves.
    preferred_x: Option<Pixels>,
    is_selecting: bool,
    /// `↩`/`↑`/`↓` edit the text; off while the host drives a popup (the
    /// composer's mention list) so its bindings get those keys.
    navigation: bool,
}

impl EventEmitter<TextAreaEvent> for TextArea {}

impl TextArea {
    pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        let placeholder = placeholder.into();
        Self {
            focus_handle: cx.focus_handle(),
            state: AreaState::default(),
            accessible_label: placeholder.clone(),
            placeholder,
            min_lines: 1,
            max_lines: 12,
            disabled: false,
            measured: Measured::default(),
            scroll_y: 0.0,
            preferred_x: None,
            is_selecting: false,
            navigation: true,
        }
    }

    /// Grows from `min` visual rows to `max`, then scrolls inside.
    pub fn set_lines(&mut self, min: usize, max: usize, cx: &mut Context<Self>) {
        self.min_lines = min.max(1);
        self.max_lines = max.max(self.min_lines);
        cx.notify();
    }

    /// See [`TextArea::navigation`]; the area keeps the keyboard either way.
    pub fn set_navigation(&mut self, navigation: bool, cx: &mut Context<Self>) {
        self.navigation = navigation;
        cx.notify();
    }

    pub fn set_accessible_label(&mut self, label: impl Into<SharedString>) {
        self.accessible_label = label.into();
    }

    pub fn set_placeholder(
        &mut self,
        placeholder: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.placeholder = placeholder.into();
        cx.notify();
    }

    pub fn text(&self) -> &str {
        self.state.text()
    }

    pub fn state(&self) -> &AreaState {
        &self.state
    }

    /// Replace text and atoms (caret at the end); emits `Changed` if the
    /// text differs.
    pub fn set_text(&mut self, text: &str, atoms: Vec<Atom>, cx: &mut Context<Self>) {
        let changed = self.state.text() != text || self.state.atoms != atoms;
        self.state.set(text, atoms);
        self.edited(changed, cx);
    }

    /// Replace `range` with an atom (mention chip) plus a trailing space.
    pub fn insert_atom(
        &mut self,
        range: Range<usize>,
        display: &str,
        id: SharedString,
        cx: &mut Context<Self>,
    ) {
        self.state.insert_atom(range, display, id);
        self.state.replace(None, " ");
        self.edited(true, cx);
    }

    pub fn serialize(&self, token: impl Fn(&str) -> String) -> String {
        self.state.serialize(token)
    }

    /// Select everything (editors open with the title selected).
    pub fn select_all_text(&mut self, cx: &mut Context<Self>) {
        self.state.edit.select_all();
        cx.notify();
    }

    fn edited(&mut self, changed: bool, cx: &mut Context<Self>) {
        self.preferred_x = None;
        if changed {
            cx.emit(TextAreaEvent::Changed);
        }
        cx.notify();
    }

    fn newline(&mut self, _: &Newline, _: &mut Window, cx: &mut Context<Self>) {
        let changed = self.state.replace(None, "\n");
        self.edited(changed, cx);
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        self.state.step(false, false);
        self.preferred_x = None;
        cx.notify();
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        self.state.step(true, false);
        self.preferred_x = None;
        cx.notify();
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.state.step(false, true);
        cx.notify();
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.state.step(true, true);
        cx.notify();
    }

    fn vertical(&mut self, rows: f32, extend: bool, cx: &mut Context<Self>) {
        let cursor = self.state.edit.cursor();
        let target = match self.measured.position(cursor) {
            Some(position) => {
                let x = *self.preferred_x.get_or_insert(position.x);
                let y = position.y + self.measured.line_height * rows;
                if y < px(0.0) {
                    0
                } else {
                    let total = self.measured.tops.last().copied().unwrap_or_default()
                        + self
                            .measured
                            .lines
                            .last()
                            .map_or(px(0.0), |l| l.size(self.measured.line_height).height);
                    if y >= total {
                        self.state.text().len()
                    } else {
                        self.measured.offset(point(x, y))
                    }
                }
            }
            None if rows < 0.0 => 0,
            None => self.state.text().len(),
        };
        let target = self.state.snap(target, rows > 0.0);
        if extend {
            self.state.edit.select_to(target);
        } else {
            self.state.edit.move_to(target);
        }
        cx.notify();
    }

    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical(-1.0, false, cx);
    }

    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical(1.0, false, cx);
    }

    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical(-1.0, true, cx);
    }

    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical(1.0, true, cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.state.edit.select_all();
        cx.notify();
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        let start = self.state.line_start(self.state.edit.cursor());
        self.state.edit.move_to(start);
        self.preferred_x = None;
        cx.notify();
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        let end = self.state.line_end(self.state.edit.cursor());
        self.state.edit.move_to(end);
        self.preferred_x = None;
        cx.notify();
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.state.edit.selection().is_empty() && self.state.edit.cursor() == 0 {
            window.play_system_bell();
            return;
        }
        let changed = self.state.backspace();
        self.edited(changed, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.state.edit.selection().is_empty()
            && self.state.edit.cursor() == self.state.text().len()
        {
            window.play_system_bell();
            return;
        }
        let changed = self.state.delete();
        self.edited(changed, cx);
    }

    fn delete_to_start(&mut self, _: &DeleteToStart, _: &mut Window, cx: &mut Context<Self>) {
        let end = self.state.edit.selection().end;
        let start = self.state.line_start(self.state.edit.selection().start);
        let changed = self.state.replace(Some(start..end), "");
        self.edited(changed, cx);
    }

    fn show_character_palette(
        &mut self,
        _: &ShowCharacterPalette,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.show_character_palette();
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let changed = self.state.replace(None, &text.replace("\r\n", "\n"));
            self.edited(changed, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        let range = self.state.edit.selection();
        if !range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.state.text()[range].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        let range = self.state.edit.selection();
        if !range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.state.text()[range].to_string(),
            ));
            let changed = self.state.replace(None, "");
            self.edited(changed, cx);
        }
    }

    /// Window position → text offset (snapped out of atoms).
    fn offset_for_mouse(&self, position: Point<Pixels>) -> usize {
        let Some(bounds) = self.measured.bounds else {
            return 0;
        };
        let local = point(
            position.x - bounds.left(),
            position.y - bounds.top() + px(self.scroll_y),
        );
        self.state.snap(self.measured.offset(local), true)
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        window.focus(&self.focus_handle, cx);
        self.is_selecting = true;
        let offset = self.offset_for_mouse(event.position);
        if event.modifiers.shift {
            self.state.edit.select_to(offset);
        } else {
            self.state.edit.move_to(offset);
        }
        self.preferred_x = None;
        cx.notify();
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            let offset = self.offset_for_mouse(event.position);
            self.state.edit.select_to(offset);
            cx.notify();
        }
    }

    /// Text runs over `text`: atoms in `link` at weight 560, the IME
    /// composition underlined.
    fn runs(&self, text_len: usize, base: &TextRun, link: Hsla) -> Vec<TextRun> {
        let marked = self.state.edit.marked();
        let mut cuts: Vec<usize> = vec![0, text_len];
        for atom in &self.state.atoms {
            cuts.extend([atom.range.start, atom.range.end]);
        }
        if let Some(marked) = &marked {
            cuts.extend([marked.start, marked.end]);
        }
        cuts.retain(|c| *c <= text_len);
        cuts.sort_unstable();
        cuts.dedup();
        cuts.windows(2)
            .map(|w| {
                let mut run = TextRun {
                    len: w[1] - w[0],
                    ..base.clone()
                };
                if self
                    .state
                    .atoms
                    .iter()
                    .any(|a| a.range.start <= w[0] && w[1] <= a.range.end)
                {
                    run.color = link;
                    run.font.weight = FontWeight(560.0);
                }
                if marked
                    .as_ref()
                    .is_some_and(|m| m.start <= w[0] && w[1] <= m.end)
                {
                    run.underline = Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    });
                }
                run
            })
            .collect()
    }
}

impl EntityInputHandler for TextArea {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.state.edit.range_from_utf16(&range_utf16);
        actual_range.replace(self.state.edit.range_to_utf16(&range));
        self.state.text().get(range).map(str::to_owned)
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let selection = self.state.edit.selection();
        let reversed = !selection.is_empty() && self.state.edit.cursor() == selection.start;
        Some(UTF16Selection {
            range: self.state.edit.range_to_utf16(&selection),
            reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.state
            .edit
            .marked()
            .map(|range| self.state.edit.range_to_utf16(&range))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        let text = self.state.text().to_owned();
        let selection = self.state.edit.selection();
        self.state.edit.set_text(&text);
        self.state.edit.move_to(selection.start);
        if !selection.is_empty() {
            self.state.edit.select_to(selection.end);
        }
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        let range = range_utf16.map(|range| self.state.edit.range_from_utf16(&range));
        let changed = self.state.replace(range, new_text);
        self.edited(changed, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        let range = range_utf16.map(|range| self.state.edit.range_from_utf16(&range));
        let selected = new_selected_range_utf16.map(|range| {
            let mut inserted = EditState::default();
            inserted.set_text(new_text);
            inserted.range_from_utf16(&range)
        });
        // Atoms first (the composition never lands inside one).
        let range = self.state.resolve(range);
        let start = self.state.snap(range.start, false);
        let end = self.state.snap(range.end, true);
        self.state.replace(Some(start..end), "");
        let changed = self
            .state
            .edit
            .replace_and_mark(Some(start..start), new_text, selected);
        for atom in &mut self.state.atoms {
            if atom.range.start >= start {
                atom.range.start += new_text.len();
                atom.range.end += new_text.len();
            }
        }
        self.edited(changed, cx);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.state.edit.range_from_utf16(&range_utf16);
        let start = self.measured.position(range.start)?;
        let end = self.measured.position(range.end)?;
        let origin = point(bounds.left(), bounds.top() - px(self.scroll_y));
        Some(Bounds::from_corners(
            origin + start,
            origin + point(end.x, end.y + self.measured.line_height),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point_in_window: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.measured.bounds?;
        if !bounds.contains(&point_in_window) {
            return None;
        }
        let offset = self.offset_for_mouse(point_in_window);
        Some(self.state.edit.offset_to_utf16(offset))
    }
}

struct AreaElement {
    area: Entity<TextArea>,
}

struct AreaPrepaint {
    lines: Vec<WrappedLine>,
    tops: Vec<Pixels>,
    starts: Vec<usize>,
    line_height: Pixels,
    cursor: Option<PaintQuad>,
    selection: Vec<PaintQuad>,
    scroll_y: f32,
}

impl IntoElement for AreaElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// Logical line starts of `text` (one more than its newlines).
fn line_starts(text: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(text.match_indices('\n').map(|(ix, _)| ix + 1))
        .collect()
}

/// Shapes `text` wrapped at `width`: lines, their tops and starts.
fn shape(
    text: &str,
    runs: &[TextRun],
    font_size: Pixels,
    line_height: Pixels,
    width: Pixels,
    window: &Window,
) -> (Vec<WrappedLine>, Vec<Pixels>, Vec<usize>) {
    let lines: Vec<WrappedLine> = window
        .text_system()
        .shape_text(
            SharedString::from(text.to_owned()),
            font_size,
            runs,
            Some(width),
            None,
        )
        .map(|lines| lines.into_iter().collect())
        .unwrap_or_default();
    let mut tops = Vec::with_capacity(lines.len());
    let mut y = px(0.0);
    for line in &lines {
        tops.push(y);
        y += line.size(line_height).height;
    }
    let mut starts = line_starts(text);
    starts.truncate(lines.len());
    (lines, tops, starts)
}

impl Element for AreaElement {
    type RequestLayoutState = ();
    type PrepaintState = AreaPrepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let area = self.area.read(cx);
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let text = if area.state.text().is_empty() {
            area.placeholder.to_string()
        } else {
            area.state.text().to_owned()
        };
        let run = TextRun {
            len: text.len(),
            font: style.font(),
            color: style.color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let (min, max) = (area.min_lines as f32, area.max_lines as f32);
        let mut layout_style = Style::default();
        layout_style.size.width = relative(1.).into();
        let id = window.request_measured_layout(
            layout_style,
            move |known: Size<Option<Pixels>>, available: Size<AvailableSpace>, window, _| {
                let width = known.width.unwrap_or(match available.width {
                    AvailableSpace::Definite(width) => width,
                    _ => px(10_000.0),
                });
                let (lines, _, _) = shape(
                    &text,
                    std::slice::from_ref(&run),
                    font_size,
                    line_height,
                    width,
                    window,
                );
                let rows: usize = lines
                    .iter()
                    .map(|line| line.wrap_boundaries().len() + 1)
                    .sum();
                let rows = (rows.max(1) as f32).clamp(min, max);
                size(width, line_height * rows)
            },
        );
        (id, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let colors = &cx.theme().colors;
        let (caret_color, selection_color, placeholder_color, link) = (
            colors.accent_solid,
            colors.accent_wash,
            colors.text_tertiary,
            colors.text_link,
        );
        let area = self.area.read(cx);
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let empty = area.state.text().is_empty();
        let base = TextRun {
            len: 0,
            font: style.font(),
            color: if empty {
                placeholder_color
            } else {
                style.color
            },
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let (text, runs) = if empty {
            let text = area.placeholder.to_string();
            let run = TextRun {
                len: text.len(),
                ..base
            };
            (text, vec![run])
        } else {
            let text = area.state.text().to_owned();
            let runs = area.runs(text.len(), &base, link);
            (text, runs)
        };
        let (lines, tops, starts) = shape(
            &text,
            &runs,
            font_size,
            line_height,
            bounds.size.width,
            window,
        );
        let measured = Measured {
            lines: Vec::new(),
            starts: starts.clone(),
            tops: tops.clone(),
            line_height,
            bounds: None,
        };
        let position = |offset: usize| -> Point<Pixels> {
            let ix = starts
                .iter()
                .rposition(|s| *s <= offset)
                .unwrap_or(0)
                .min(lines.len().saturating_sub(1));
            lines
                .get(ix)
                .and_then(|line| {
                    line.position_for_index((offset - starts[ix]).min(line.len()), line_height)
                })
                .map_or(point(px(0.0), px(0.0)), |p| {
                    point(p.x, p.y + measured.tops[ix])
                })
        };
        let content_height = tops.last().copied().unwrap_or_default()
            + lines
                .last()
                .map_or(line_height, |l| l.size(line_height).height);

        let selected = area.state.edit.selection();
        let caret = if empty {
            point(px(0.0), px(0.0))
        } else {
            position(area.state.edit.cursor())
        };
        // Keep the caret's row inside the visible box.
        let viewport = f32::from(bounds.size.height);
        let mut scroll_y = area.scroll_y;
        let (top, bottom) = (f32::from(caret.y), f32::from(caret.y + line_height));
        if bottom > scroll_y + viewport {
            scroll_y = bottom - viewport;
        }
        if top < scroll_y {
            scroll_y = top;
        }
        scroll_y = scroll_y.clamp(0.0, (f32::from(content_height) - viewport).max(0.0));
        let origin = point(bounds.left(), bounds.top() - px(scroll_y));

        let (selection, cursor) = if selected.is_empty() || empty {
            let quad = fill(
                Bounds::new(origin + caret, size(px(CARET_WIDTH), line_height)),
                caret_color,
            );
            (Vec::new(), Some(quad))
        } else {
            let (start, end) = (position(selected.start), position(selected.end));
            let right = bounds.size.width;
            let mut quads = Vec::new();
            if start.y == end.y {
                quads.push(Bounds::from_corners(
                    start,
                    point(end.x, end.y + line_height),
                ));
            } else {
                quads.push(Bounds::from_corners(
                    start,
                    point(right, start.y + line_height),
                ));
                if end.y > start.y + line_height {
                    quads.push(Bounds::from_corners(
                        point(px(0.0), start.y + line_height),
                        point(right, end.y),
                    ));
                }
                quads.push(Bounds::from_corners(
                    point(px(0.0), end.y),
                    point(end.x, end.y + line_height),
                ));
            }
            let quads = quads
                .into_iter()
                .map(|b| fill(Bounds::new(origin + b.origin, b.size), selection_color))
                .collect();
            (quads, None)
        };
        AreaPrepaint {
            lines,
            tops,
            starts,
            line_height,
            cursor,
            selection,
            scroll_y,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.area.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.area.clone()),
            cx,
        );
        let focused = focus_handle.is_focused(window);
        let origin = point(bounds.left(), bounds.top() - px(prepaint.scroll_y));
        let line_height = prepaint.line_height;
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for quad in prepaint.selection.drain(..) {
                window.paint_quad(quad);
            }
            for (line, top) in prepaint.lines.iter().zip(&prepaint.tops) {
                // Painting only fails on glyph rasterization errors; a
                // missing frame of text beats crashing the editor.
                line.paint(
                    point(origin.x, origin.y + *top),
                    line_height,
                    gpui::TextAlign::Left,
                    None,
                    window,
                    cx,
                )
                .ok();
            }
            if focused && let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        });
        let lines = std::mem::take(&mut prepaint.lines);
        let (tops, starts, scroll_y) = (
            std::mem::take(&mut prepaint.tops),
            std::mem::take(&mut prepaint.starts),
            prepaint.scroll_y,
        );
        let empty = self.area.read(cx).state.text().is_empty();
        self.area.update(cx, |area, _| {
            area.measured = if empty {
                Measured {
                    line_height,
                    bounds: Some(bounds),
                    ..Measured::default()
                }
            } else {
                Measured {
                    lines,
                    starts,
                    tops,
                    line_height,
                    bounds: Some(bounds),
                }
            };
            area.scroll_y = scroll_y;
        });
    }
}

impl Render for TextArea {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("text-area")
            .flex()
            .w_full()
            .key_context(if self.navigation {
                "TextInput TextArea"
            } else {
                "TextInput"
            })
            .track_focus(&self.focus_handle)
            .role(gpui::Role::MultilineTextInput)
            .aria_label(self.accessible_label.clone())
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::newline))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::delete_to_start))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::show_character_palette))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .child(AreaElement { area: cx.entity() })
    }
}

impl Focusable for TextArea {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_chip() -> AreaState {
        let mut state = AreaState::default();
        state.set("hi ", Vec::new());
        state.insert_atom(3..3, "@Dana Ruiz", "d291".into());
        state.replace(None, " please");
        state
    }

    #[test]
    fn atoms_serialize_as_their_tokens() {
        let state = with_chip();
        assert_eq!(state.text(), "hi @Dana Ruiz please");
        assert_eq!(
            state.serialize(|id| format!("@<{id}>")),
            "hi @<d291> please"
        );
    }

    #[test]
    fn backspace_after_an_atom_deletes_it_whole() {
        let mut state = with_chip();
        state.edit.move_to(13);
        assert!(state.backspace());
        assert_eq!(state.text(), "hi  please");
        assert!(state.atoms().is_empty());
    }

    #[test]
    fn caret_steps_over_an_atom_as_one_character() {
        let mut state = with_chip();
        state.edit.move_to(3);
        state.step(true, false);
        assert_eq!(state.edit.cursor(), 13);
        state.step(false, false);
        assert_eq!(state.edit.cursor(), 3);
    }

    #[test]
    fn typing_before_an_atom_shifts_it() {
        let mut state = with_chip();
        state.edit.move_to(0);
        state.replace(None, "oh ");
        assert_eq!(state.atoms()[0].range, 6..16);
        assert_eq!(
            state.serialize(|id| format!("@<{id}>")),
            "oh hi @<d291> please"
        );
    }

    #[test]
    fn a_selection_reaching_into_an_atom_removes_it() {
        let mut state = with_chip();
        state.edit.move_to(0);
        state.edit.select_to(5);
        state.replace(None, "");
        assert_eq!(state.text(), " please");
        assert!(state.atoms().is_empty());
    }

    #[test]
    fn line_bounds_and_text_before_the_caret() {
        let mut state = AreaState::default();
        state.set("one\ntwo @da", Vec::new());
        assert_eq!(state.line_start(6), 4);
        assert_eq!(state.line_end(1), 3);
        assert_eq!(state.before_caret_on_line(), (4, "two @da"));
    }

    #[test]
    fn line_starts_follow_newlines() {
        assert_eq!(line_starts("a\nbc\n"), vec![0, 2, 5]);
        assert_eq!(line_starts(""), vec![0]);
    }
}
