//! Command gates (command-palette.md §4.3, spec 0015 FR-28): a view
//! registers, per action, why its `⌘P` command can't run right now. The
//! palette lists a gated command in the disabled style with the reason as
//! its subtitle, and confirming it shows the reason instead of running it.
//!
//! One registry instead of a field on every [`super::PaletteCommand`]
//! literal: gates are rare and live with the view that owns the action.

use std::collections::HashMap;

use gpui::{Action, App, Global, SharedString};

/// Why the command can't run now (`None`: it can).
pub type Gate = fn(&App) -> Option<SharedString>;

#[derive(Default)]
struct Gates(HashMap<&'static str, Gate>);

impl Global for Gates {}

/// Gate the command running `action` (by its action name); a later
/// registration for the same action replaces the earlier one.
pub fn register(action: &dyn Action, gate: Gate, cx: &mut App) {
    cx.default_global::<Gates>().0.insert(action.name(), gate);
}

/// Why `action`'s command can't run now, if it's gated.
pub fn blocked(action: &dyn Action, cx: &App) -> Option<SharedString> {
    let gate = cx.try_global::<Gates>()?.0.get(action.name())?;
    gate(cx)
}
