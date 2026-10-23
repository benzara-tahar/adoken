//! Single-line text input (the Cmd+P query, the PAT paste field).
//!
//! Masked mode ([`TextInput::set_masked`]) is for secrets: it paints one
//! bullet per character, answers IME/accessibility text queries with
//! bullets, refuses copy/cut, and zeroizes the buffer when it is replaced or
//! dropped.
//!
//! Text longer than the input scrolls horizontally so the caret (and any IME
//! composition) stays in view; painting is clipped to the input's bounds.
//!
//! Adapted from GPUI's `crates/gpui/examples/input.rs` (zui rev `18a89af`,
//! Apache-2.0): the IME-aware `EntityInputHandler` and the custom text
//! element. The editing model is split out as [`EditState`] so it is unit
//! tested without a window, and every color comes from the theme.
//!
//! Register [`bindings`] once at startup; the input handles them while it
//! (or a child) is focused, in the [`KEY_CONTEXT`] key context.

use std::ops::Range;

use adoken_theme::space;
use gpui::{
    App, Bounds, ClipboardItem, ContentMask, Context, CursorStyle, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable, GlobalElementId, KeyBinding,
    LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point,
    ShapedLine, SharedString, Style, TextRun, UTF16Selection, UnderlineStyle, Window, actions, div,
    fill, point, prelude::*, px, relative, size,
};
use unicode_segmentation::UnicodeSegmentation;
use zeroize::Zeroize;

use crate::theme::ActiveTheme;

actions!(
    text_input,
    [
        /// Delete the previous grapheme or the selection.
        Backspace,
        /// Delete the next grapheme or the selection.
        Delete,
        /// Delete from the start of the line to the caret.
        DeleteToStart,
        /// Move the caret one grapheme left.
        Left,
        /// Move the caret one grapheme right.
        Right,
        /// Extend the selection one grapheme left.
        SelectLeft,
        /// Extend the selection one grapheme right.
        SelectRight,
        /// Select all text.
        SelectAll,
        /// Move the caret to the start.
        Home,
        /// Move the caret to the end.
        End,
        /// Open the system emoji and symbol picker.
        ShowCharacterPalette,
        /// Paste plain text (newlines become spaces).
        Paste,
        /// Cut the selection.
        Cut,
        /// Copy the selection.
        Copy,
    ]
);

/// Key context the input's bindings are scoped to.
pub const KEY_CONTEXT: &str = "TextInput";

/// Default bindings for every [`TextInput`]; `secondary` is ⌘ on macOS and
/// Ctrl elsewhere.
pub fn bindings() -> Vec<KeyBinding> {
    let cx = Some(KEY_CONTEXT);
    let mut bindings = vec![
        KeyBinding::new("backspace", Backspace, cx),
        KeyBinding::new("delete", Delete, cx),
        KeyBinding::new("secondary-backspace", DeleteToStart, cx),
        KeyBinding::new("left", Left, cx),
        KeyBinding::new("right", Right, cx),
        KeyBinding::new("shift-left", SelectLeft, cx),
        KeyBinding::new("shift-right", SelectRight, cx),
        KeyBinding::new("secondary-a", SelectAll, cx),
        KeyBinding::new("home", Home, cx),
        KeyBinding::new("end", End, cx),
        KeyBinding::new("secondary-v", Paste, cx),
        KeyBinding::new("secondary-x", Cut, cx),
        KeyBinding::new("secondary-c", Copy, cx),
    ];
    if cfg!(target_os = "macos") {
        bindings.extend([
            KeyBinding::new("cmd-left", Home, cx),
            KeyBinding::new("cmd-right", End, cx),
            KeyBinding::new("ctrl-cmd-space", ShowCharacterPalette, cx),
        ]);
    }
    bindings
}

/// The text, selection and IME composition range, in UTF-8 byte offsets.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditState {
    content: String,
    selected: Range<usize>,
    reversed: bool,
    marked: Option<Range<usize>>,
}

impl EditState {
    pub fn text(&self) -> &str {
        &self.content
    }

    pub fn selection(&self) -> Range<usize> {
        self.selected.clone()
    }

    pub fn marked(&self) -> Option<Range<usize>> {
        self.marked.clone()
    }

    /// The end of the selection the caret sits on.
    pub fn cursor(&self) -> usize {
        if self.reversed {
            self.selected.start
        } else {
            self.selected.end
        }
    }

    /// Replace everything and put the caret at the end.
    pub fn set_text(&mut self, text: &str) {
        self.content.clear();
        self.content.push_str(text);
        self.selected = text.len()..text.len();
        self.reversed = false;
        self.marked = None;
    }

    pub fn move_to(&mut self, offset: usize) {
        let offset = offset.min(self.content.len());
        self.selected = offset..offset;
        self.reversed = false;
    }

    /// Move the caret end of the selection, keeping the anchor.
    pub fn select_to(&mut self, offset: usize) {
        let offset = offset.min(self.content.len());
        if self.reversed {
            self.selected.start = offset;
        } else {
            self.selected.end = offset;
        }
        if self.selected.end < self.selected.start {
            self.reversed = !self.reversed;
            self.selected = self.selected.end..self.selected.start;
        }
    }

    pub fn select_all(&mut self) {
        self.selected = 0..self.content.len();
        self.reversed = false;
    }

    /// Byte offset of the grapheme boundary before `offset` (0 at the start).
    pub fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    /// Byte offset of the grapheme boundary after `offset` (len at the end).
    pub fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len())
    }

    /// Replace `range` (default: the IME composition, else the selection)
    /// with `text`, clearing the composition. Returns whether the text changed.
    pub fn replace(&mut self, range: Option<Range<usize>>, text: &str) -> bool {
        let range = self.resolve(range);
        let changed = &self.content[range.clone()] != text;
        self.content.replace_range(range.clone(), text);
        let caret = range.start + text.len();
        self.selected = caret..caret;
        self.reversed = false;
        self.marked = None;
        changed
    }

    /// IME composition: replace `range` with `text`, mark it, and place the
    /// selection at `selected` (relative to the inserted text).
    pub fn replace_and_mark(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
    ) -> bool {
        let range = self.resolve(range);
        let changed = &self.content[range.clone()] != text;
        self.content.replace_range(range.clone(), text);
        self.marked = (!text.is_empty()).then(|| range.start..range.start + text.len());
        self.selected = selected
            .map(|sel| range.start + sel.start..range.start + sel.end)
            .unwrap_or_else(|| {
                let caret = range.start + text.len();
                caret..caret
            });
        self.reversed = false;
        changed
    }

    fn resolve(&self, range: Option<Range<usize>>) -> Range<usize> {
        let range = range
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.selected.clone());
        let end = range.end.min(self.content.len());
        range.start.min(end)..end
    }

    pub fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8 = 0;
        let mut utf16 = 0;
        for ch in self.content.chars() {
            if utf16 >= offset {
                break;
            }
            utf16 += ch.len_utf16();
            utf8 += ch.len_utf8();
        }
        utf8
    }

    pub fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16 = 0;
        let mut utf8 = 0;
        for ch in self.content.chars() {
            if utf8 >= offset {
                break;
            }
            utf8 += ch.len_utf8();
            utf16 += ch.len_utf16();
        }
        utf16
    }

    pub fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    pub fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }

    /// The masked rendering: one [`MASK`] per UTF-16 unit, so UTF-16 offsets
    /// into the real text and into the mask agree.
    pub fn masked_text(&self) -> String {
        MASK.to_string()
            .repeat(self.offset_to_utf16(self.content.len()))
    }

    /// Byte offset in [`EditState::masked_text`] for byte `offset` in the text.
    pub fn masked_offset(&self, offset: usize) -> usize {
        self.offset_to_utf16(offset) * MASK.len_utf8()
    }

    /// Byte offset in the text for byte `offset` in the masked text.
    pub fn unmasked_offset(&self, offset: usize) -> usize {
        self.offset_from_utf16(offset / MASK.len_utf8())
    }
}

/// Glyph painted for each character of a masked input.
pub const MASK: char = '•';

/// Width of the painted caret, in px.
const CARET_WIDTH: f32 = 2.0;

/// Room kept between the caret and either edge while it moves through
/// overflowing text, so the neighbouring characters stay readable.
const SCROLL_MARGIN: f32 = space::S8;

/// Horizontal scroll (how far the line is shifted left, in px) that brings
/// `target` (a content-x range: the caret or the IME composition) inside a
/// `viewport`-wide box, moving as little as possible from `current`.
///
/// The result is clamped to `0..=line_width + caret − viewport`, so the line
/// never scrolls past its end (a caret after the last character stays fully
/// visible) and text that shrinks or fits again slides back.
fn scroll_to_reveal(current: f32, target: Range<f32>, line_width: f32, viewport: f32) -> f32 {
    // A box narrower than target + both margins shrinks the margin first.
    let margin = SCROLL_MARGIN.min(((viewport - (target.end - target.start)) / 2.0).max(0.0));
    let mut next = current;
    if target.end + margin > next + viewport {
        next = target.end + margin - viewport;
    }
    // The start wins when the target is wider than the box.
    if target.start - margin < next {
        next = target.start - margin;
    }
    next.clamp(0.0, (line_width + CARET_WIDTH - viewport).max(0.0))
}

/// Pointer x in window coordinates → x along the shaped line.
fn content_x(pointer_x: Pixels, left: Pixels, scroll: f32) -> Pixels {
    pointer_x - left + px(scroll)
}

/// Emitted after every change to the text (typing, paste, IME, `set_text`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextInputEvent {
    Changed,
    /// Clipboard text was pasted (after the matching `Changed`); lets a
    /// token field verify on paste without waiting for Enter.
    Pasted,
}

pub struct TextInput {
    focus_handle: FocusHandle,
    state: EditState,
    placeholder: SharedString,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    /// Horizontal scroll painted last frame, in px ([`scroll_to_reveal`]).
    scroll_x: f32,
    is_selecting: bool,
    masked: bool,
}

impl EventEmitter<TextInputEvent> for TextInput {}

impl TextInput {
    pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            state: EditState::default(),
            placeholder: placeholder.into(),
            last_layout: None,
            last_bounds: None,
            scroll_x: 0.0,
            is_selecting: false,
            masked: false,
        }
    }

    /// Secret entry: bullets on screen and to the platform, no copy/cut,
    /// zeroized storage.
    pub fn set_masked(&mut self, masked: bool, cx: &mut Context<Self>) {
        self.masked = masked;
        cx.notify();
    }

    pub fn is_masked(&self) -> bool {
        self.masked
    }

    /// Text offset → offset in what is painted.
    fn display_offset(&self, offset: usize) -> usize {
        if self.masked {
            self.state.masked_offset(offset)
        } else {
            offset
        }
    }

    /// Painted offset → text offset.
    fn text_offset(&self, offset: usize) -> usize {
        if self.masked {
            self.state.unmasked_offset(offset)
        } else {
            offset
        }
    }

    pub fn text(&self) -> &str {
        self.state.text()
    }

    /// Replace the text (caret at the end); emits `Changed` if it differs.
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        let changed = self.state.text() != text;
        if self.masked {
            self.state.content.zeroize();
        }
        self.state.set_text(text);
        if changed {
            cx.emit(TextInputEvent::Changed);
        }
        cx.notify();
    }

    /// Select everything (an inline editor opens with its value selected).
    pub fn select_all_text(&mut self, cx: &mut Context<Self>) {
        self.state.select_all();
        cx.notify();
    }

    fn edited(&mut self, changed: bool, cx: &mut Context<Self>) {
        if changed {
            cx.emit(TextInputEvent::Changed);
        }
        cx.notify();
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        let target = if self.state.selected.is_empty() {
            self.state.previous_boundary(self.state.cursor())
        } else {
            self.state.selected.start
        };
        self.state.move_to(target);
        cx.notify();
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        let target = if self.state.selected.is_empty() {
            self.state.next_boundary(self.state.cursor())
        } else {
            self.state.selected.end
        };
        self.state.move_to(target);
        cx.notify();
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.state
            .select_to(self.state.previous_boundary(self.state.cursor()));
        cx.notify();
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.state
            .select_to(self.state.next_boundary(self.state.cursor()));
        cx.notify();
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.state.select_all();
        cx.notify();
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.state.move_to(0);
        cx.notify();
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.state.move_to(self.state.text().len());
        cx.notify();
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.state.selected.is_empty() {
            let cursor = self.state.cursor();
            let prev = self.state.previous_boundary(cursor);
            if cursor == prev {
                window.play_system_bell();
                return;
            }
            self.state.select_to(prev);
        }
        let changed = self.state.replace(None, "");
        self.edited(changed, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.state.selected.is_empty() {
            let cursor = self.state.cursor();
            let next = self.state.next_boundary(cursor);
            if cursor == next {
                window.play_system_bell();
                return;
            }
            self.state.select_to(next);
        }
        let changed = self.state.replace(None, "");
        self.edited(changed, cx);
    }

    fn delete_to_start(&mut self, _: &DeleteToStart, _: &mut Window, cx: &mut Context<Self>) {
        let end = self.state.selected.end;
        let changed = self.state.replace(Some(0..end), "");
        self.edited(changed, cx);
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.is_selecting = true;
        let offset = self.index_for_mouse_position(event.position);
        if event.modifiers.shift {
            self.state.select_to(offset);
        } else {
            self.state.move_to(offset);
        }
        cx.notify();
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.state
                .select_to(self.index_for_mouse_position(event.position));
            cx.notify();
        }
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
            let changed = self.state.replace(None, &text.replace(['\n', '\r'], " "));
            self.edited(changed, cx);
            cx.emit(TextInputEvent::Pasted);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if self.masked {
            return;
        }
        let range = self.state.selection();
        if !range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.state.text()[range].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if self.masked {
            return;
        }
        let range = self.state.selection();
        if !range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.state.text()[range].to_string(),
            ));
            let changed = self.state.replace(None, "");
            self.edited(changed, cx);
        }
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.state.text().is_empty() {
            return 0;
        }
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.state.text().len();
        }
        self.text_offset(line.closest_index_for_x(content_x(
            position.x,
            bounds.left(),
            self.scroll_x,
        )))
    }
}

impl Drop for TextInput {
    fn drop(&mut self) {
        if self.masked {
            self.state.content.zeroize();
        }
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.state.range_from_utf16(&range_utf16);
        let utf16 = self.state.range_to_utf16(&range);
        actual_range.replace(utf16.clone());
        if self.masked {
            return Some(MASK.to_string().repeat(utf16.len()));
        }
        self.state.text().get(range).map(str::to_owned)
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.state.range_to_utf16(&self.state.selected),
            reversed: self.state.reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.state
            .marked
            .as_ref()
            .map(|range| self.state.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.state.marked = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16.map(|range| self.state.range_from_utf16(&range));
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
        let range = range_utf16.map(|range| self.state.range_from_utf16(&range));
        // The selection is relative to `new_text`, so convert against it.
        let selected = new_selected_range_utf16.map(|range| {
            let mut inserted = EditState::default();
            inserted.set_text(new_text);
            inserted.range_from_utf16(&range)
        });
        let changed = self.state.replace_and_mark(range, new_text, selected);
        self.edited(changed, cx);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let layout = self.last_layout.as_ref()?;
        let range = self.state.range_from_utf16(&range_utf16);
        let (start, end) = (
            self.display_offset(range.start),
            self.display_offset(range.end),
        );
        let left = bounds.left() - px(self.scroll_x);
        Some(Bounds::from_corners(
            point(left + layout.x_for_index(start), bounds.top()),
            point(left + layout.x_for_index(end), bounds.bottom()),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.last_bounds?;
        if !bounds.contains(&point) {
            return None;
        }
        let layout = self.last_layout.as_ref()?;
        let x = content_x(point.x, bounds.left(), self.scroll_x);
        let utf8 = self.text_offset(layout.index_for_x(x)?);
        Some(self.state.offset_to_utf16(utf8))
    }
}

struct TextElement {
    input: Entity<TextInput>,
}

struct PrepaintState {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
    scroll_x: f32,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

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
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
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
        let (caret_color, selection_color, placeholder_color) = (
            colors.accent_solid,
            colors.accent_wash,
            colors.text_tertiary,
        );
        let input = self.input.read(cx);
        let state = &input.state;
        let style = window.text_style();

        let (display_text, text_color): (SharedString, _) = if state.text().is_empty() {
            (input.placeholder.clone(), placeholder_color)
        } else if input.masked {
            (state.masked_text().into(), style.color)
        } else {
            (state.text().to_owned().into(), style.color)
        };
        let marked = state
            .marked
            .as_ref()
            .map(|m| input.display_offset(m.start)..input.display_offset(m.end));

        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = match marked.as_ref() {
            Some(marked) if !state.text().is_empty() => [
                TextRun {
                    len: marked.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked.end - marked.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: display_text.len() - marked.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect(),
            _ => vec![run],
        };

        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(display_text, font_size, &runs, None);

        // Keep the IME composition, then the caret (which wins), in view.
        let scroll_x = if state.text().is_empty() {
            0.0
        } else {
            let (line_width, viewport) = (f32::from(line.width()), f32::from(bounds.size.width));
            let x = |offset| f32::from(line.x_for_index(offset));
            let caret = x(input.display_offset(state.cursor()));
            let scroll = marked.as_ref().map_or(input.scroll_x, |marked| {
                scroll_to_reveal(
                    input.scroll_x,
                    x(marked.start)..x(marked.end),
                    line_width,
                    viewport,
                )
            });
            scroll_to_reveal(scroll, caret..caret + CARET_WIDTH, line_width, viewport)
        };
        let left = bounds.left() - px(scroll_x);

        let selected = state.selection();
        let (selection, cursor) = if selected.is_empty() {
            // Placeholder shown: the caret sits at the start, not after it.
            let x = if state.text().is_empty() {
                px(0.0)
            } else {
                line.x_for_index(input.display_offset(state.cursor()))
            };
            let caret = fill(
                Bounds::new(
                    point(left + x, bounds.top()),
                    size(px(CARET_WIDTH), bounds.bottom() - bounds.top()),
                ),
                caret_color,
            );
            (None, Some(caret))
        } else {
            let quad = fill(
                Bounds::from_corners(
                    point(
                        left + line.x_for_index(input.display_offset(selected.start)),
                        bounds.top(),
                    ),
                    point(
                        left + line.x_for_index(input.display_offset(selected.end)),
                        bounds.bottom(),
                    ),
                ),
                selection_color,
            );
            (Some(quad), None)
        };
        PrepaintState {
            line: Some(line),
            cursor,
            selection,
            scroll_x,
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
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        let Some(line) = prepaint.line.take() else {
            return;
        };
        let focused = focus_handle.is_focused(window);
        let origin = point(bounds.left() - px(prepaint.scroll_x), bounds.top());
        // Overflowing text scrolls; nothing paints past the input's edges.
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            if let Some(selection) = prepaint.selection.take() {
                window.paint_quad(selection);
            }
            // Painting a shaped line only fails on glyph rasterization
            // errors; a missing frame of text beats crashing the input.
            line.paint(
                origin,
                window.line_height(),
                gpui::TextAlign::Left,
                None,
                window,
                cx,
            )
            .ok();
            if focused && let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        });

        let scroll_x = prepaint.scroll_x;
        self.input.update(cx, |input, _| {
            input.last_layout = Some(line);
            input.last_bounds = Some(bounds);
            input.scroll_x = scroll_x;
        });
    }
}

impl Render for TextInput {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .w_full()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
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
            .child(TextElement { input: cx.entity() })
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(text: &str) -> EditState {
        let mut state = EditState::default();
        state.set_text(text);
        state
    }

    #[test]
    fn grapheme_boundaries_skip_whole_clusters() {
        // "e" + combining acute is one grapheme of 3 bytes; 👍🏽 is one of 8.
        let s = state("ae\u{301}👍🏽b");
        assert_eq!(s.next_boundary(0), 1);
        assert_eq!(s.next_boundary(1), 4);
        assert_eq!(s.next_boundary(4), 12);
        assert_eq!(s.previous_boundary(12), 4);
        assert_eq!(s.previous_boundary(0), 0);
        assert_eq!(s.next_boundary(13), 13);
    }

    #[test]
    fn select_to_flips_when_crossing_the_anchor() {
        let mut s = state("hello");
        s.move_to(3);
        s.select_to(5);
        assert_eq!((s.selection(), s.cursor()), (3..5, 5));
        s.select_to(1);
        assert_eq!((s.selection(), s.cursor()), (1..3, 1));
    }

    #[test]
    fn replace_uses_marked_then_selection_and_reports_change() {
        let mut s = state("retry");
        s.move_to(0);
        s.select_to(2);
        assert!(s.replace(None, "XY"));
        assert_eq!(s.text(), "XYtry");
        assert_eq!(s.selection(), 2..2);
        // Replacing with identical text is not a change.
        assert!(!s.replace(Some(0..2), "XY"));

        // IME: composition marks the inserted text; the next commit replaces it.
        let mut s = state("a");
        assert!(s.replace_and_mark(None, "ka", None));
        assert_eq!((s.text(), s.marked()), ("aka", Some(1..3)));
        assert!(s.replace(None, "か"));
        assert_eq!((s.text(), s.marked()), ("aか", None));
    }

    #[test]
    fn out_of_range_edits_are_clamped() {
        let mut s = state("ab");
        s.move_to(10);
        assert_eq!(s.cursor(), 2);
        assert!(s.replace(Some(5..9), "c"));
        assert_eq!(s.text(), "abc");
    }

    #[test]
    fn masked_offsets_map_every_caret_position_both_ways() {
        let s = state("a😀b");
        assert_eq!(s.masked_text(), "••••");
        // Byte offsets at char boundaries: 0, 1 (after a), 5 (after 😀), 6.
        for (text, masked) in [(0, 0), (1, 3), (5, 9), (6, 12)] {
            assert_eq!(s.masked_offset(text), masked);
            assert_eq!(s.unmasked_offset(masked), text);
        }
    }

    #[test]
    fn utf16_offsets_round_trip_through_astral_chars() {
        let s = state("a😀b");
        assert_eq!(s.offset_to_utf16(5), 3);
        assert_eq!(s.offset_from_utf16(3), 5);
        assert_eq!(s.range_from_utf16(&(1..3)), 1..5);
    }

    /// A monospaced line for the scroll tests: every glyph is 10 px wide,
    /// the box is 100 px, and the caret is `CARET_WIDTH` wide.
    const GLYPH: f32 = 10.0;
    const BOX: f32 = 100.0;

    /// Scroll after the caret moves to `index` in a `len`-glyph line.
    fn reveal(current: f32, index: usize, len: usize) -> f32 {
        let caret = index as f32 * GLYPH;
        scroll_to_reveal(current, caret..caret + CARET_WIDTH, len as f32 * GLYPH, BOX)
    }

    /// The caret's x inside the box once `scroll` is applied.
    fn on_screen(index: usize, scroll: f32) -> f32 {
        index as f32 * GLYPH - scroll
    }

    #[test]
    fn caret_past_the_right_edge_scrolls_it_into_view() {
        // Short text never scrolls.
        assert_eq!(reveal(0.0, 5, 5), 0.0);
        // Mid-line past the edge: the caret lands the margin inside it.
        let scroll = reveal(0.0, 25, 30);
        assert_eq!(on_screen(25, scroll) + CARET_WIDTH + SCROLL_MARGIN, BOX);
        // Typing at the end: the line's end sits flush with the edge and
        // the whole caret stays visible (no scrolling into empty space).
        let scroll = reveal(scroll, 30, 30);
        assert_eq!(on_screen(30, scroll) + CARET_WIDTH, BOX);
        // Moving within the visible part does not scroll.
        assert_eq!(reveal(scroll, 25, 30), scroll);
        // A box narrower than caret + margins still shows the caret.
        let tiny = scroll_to_reveal(0.0, 250.0..252.0, 300.0, 6.0);
        assert!((0.0..=6.0 - CARET_WIDTH).contains(&(250.0 - tiny)));
    }

    #[test]
    fn home_scrolls_back_to_the_start() {
        let end = reveal(0.0, 30, 30);
        assert!(end > 0.0);
        assert_eq!(reveal(end, 0, 30), 0.0);
        // Stepping left past the edge reveals with the margin, not by
        // jumping to the start.
        let back = reveal(end, 18, 30);
        assert_eq!(on_screen(18, back), SCROLL_MARGIN);
    }

    #[test]
    fn deleting_text_scrolls_back_as_the_line_shrinks() {
        let end = reveal(0.0, 30, 30);
        // Backspace at the end: the end stays flush with the edge.
        let scroll = reveal(end, 25, 25);
        assert_eq!(on_screen(25, scroll) + CARET_WIDTH, BOX);
        assert!(scroll < end);
        // Deleting after a mid-line caret leaves no gap past the line's end.
        let scroll = reveal(end, 22, 24);
        assert_eq!(on_screen(24, scroll) + CARET_WIDTH, BOX);
        // Once the text fits again it is unscrolled.
        assert_eq!(reveal(scroll, 8, 8), 0.0);
    }

    #[test]
    fn dragging_a_selection_past_the_edge_follows_the_pointer() {
        let left = px(40.0);
        let len = 30;
        let index_at = |pointer: f32, scroll: f32| {
            let x = f32::from(content_x(left + px(pointer), left, scroll));
            ((x / GLYPH).round() as usize).min(len)
        };
        let mut s = state(&"x".repeat(len));
        s.move_to(0);
        // Drag 30 px past the right edge: selects text beyond the box...
        let mut scroll = 0.0;
        s.select_to(index_at(BOX + 30.0, scroll));
        assert_eq!(s.selection(), 0..13);
        scroll = reveal(scroll, s.cursor(), len);
        assert!(on_screen(s.cursor(), scroll) < BOX);
        // ...and holding it there keeps extending as the line scrolls.
        s.select_to(index_at(BOX + 30.0, scroll));
        assert_eq!(s.selection(), 0..17);
        scroll = reveal(scroll, s.cursor(), len);
        // Back inside the box, the pointer maps through the scroll.
        s.select_to(index_at(50.0, scroll));
        assert_eq!(s.cursor(), ((50.0 + scroll) / GLYPH).round() as usize);
        assert_eq!(reveal(scroll, s.cursor(), len), scroll);
    }
}
