use crate::pins::{PinKind, Pins};
use adoken_model::PinSubject;
use adoken_ui::action_menu::MenuItem;
use gpui::{App, SharedString, Window};
gpui::actions!(pin_actions, [OpenRowMenu]);

pub fn bindings() -> Vec<gpui::KeyBinding> {
    vec![
        gpui::KeyBinding::new("secondary-k", OpenRowMenu, Some("Workspace")),
        gpui::KeyBinding::new("secondary-k", OpenRowMenu, Some("Palette")),
    ]
}

pub fn observe<V: 'static>(cx: &mut gpui::Context<V>) -> Option<gpui::Subscription> {
    Pins::global(cx).map(|pins| cx.observe(&pins, |_, _, cx| cx.notify()))
}

pub fn button(subject: PinSubject, title: SharedString, cx: &App) -> adoken_ui::Button {
    let pinned = Pins::global(cx).is_some_and(|pins| pins.read(cx).is_pinned(&subject));
    let reason = if pinned {
        None
    } else {
        Pins::can_pin(PinKind::of(&subject), cx).err()
    };
    let (kind, id) = match &subject {
        PinSubject::Pr(id) => ("pull request", id.to_string()),
        PinSubject::WorkItem(id) => ("work item", id.to_string()),
        PinSubject::Definition { id, .. } => ("pipeline", id.to_string()),
    };
    let tooltip = if pinned {
        "Unpin ⌘D"
    } else if matches!(&subject, PinSubject::Definition { .. }) {
        "Pin ⌘D · failures notify you (P2)"
    } else {
        "Pin ⌘D"
    };
    adoken_ui::Button::new("header-pin", if pinned { "Unpin" } else { "Pin" })
        .aria_pressed(pinned)
        .aria_label(format!(
            "{} {kind} {id}, {title}",
            if pinned { "Unpin" } else { "Pin" }
        ))
        .variant(adoken_ui::ButtonVariant::Ghost)
        .icon(if pinned {
            adoken_ui::IconName::PinOff
        } else {
            adoken_ui::IconName::Pin
        })
        .disabled(reason.is_some())
        .tooltip(reason.unwrap_or_else(|| tooltip.into()))
        .on_press(move |window, cx| toggle(Some(subject.clone()), title.clone(), window, cx))
}

pub fn action(label: &'static str, action: impl gpui::Action + Clone + 'static) -> MenuItem {
    MenuItem::new(label, move |window, cx| {
        window.dispatch_action(Box::new(action.clone()), cx)
    })
}

/// Palette presence deliberately blocks fallback, even for unsupported rows.
pub fn resolve_subject(
    palette: Option<Option<PinSubject>>,
    focused: Option<PinSubject>,
    detail: Option<PinSubject>,
) -> Option<PinSubject> {
    match palette {
        Some(subject) => subject,
        None => focused.or(detail),
    }
}

pub fn toggle(
    subject: Option<PinSubject>,
    title: impl Into<SharedString>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(subject) = subject else {
        Pins::not_pinnable_hint(window, cx);
        return;
    };
    if let Some(pins) = Pins::global(cx) {
        pins.update(cx, |pins, cx| pins.toggle(subject, title.into(), cx));
    }
}

pub fn mark_opened(subject: PinSubject, cx: &mut App) {
    if let Some(pins) = Pins::global(cx) {
        pins.update(cx, |pins, cx| pins.mark_opened(&subject, cx));
    }
}

pub fn menu_item(subject: Option<PinSubject>, title: SharedString, cx: &App) -> MenuItem {
    let pinned = subject.as_ref().is_some_and(|subject| {
        Pins::global(cx).is_some_and(|pins| pins.read(cx).is_pinned(subject))
    });
    let reason = if pinned {
        None
    } else {
        subject.as_ref().map_or_else(
            || {
                Some(SharedString::from(
                    "Only pull requests, work items and pipelines can be pinned",
                ))
            },
            |subject| Pins::can_pin(PinKind::of(subject), cx).err(),
        )
    };
    MenuItem::new(if pinned { "Unpin" } else { "Pin" }, move |window, cx| {
        toggle(subject.clone(), title.clone(), window, cx)
    })
    .keys("secondary-d")
    .disabled(reason)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn palette_selection_wins_and_unsupported_selection_blocks_detail() {
        assert_eq!(
            resolve_subject(
                Some(Some(PinSubject::Pr(3))),
                Some(PinSubject::WorkItem(2)),
                Some(PinSubject::Pr(1))
            ),
            Some(PinSubject::Pr(3))
        );
        assert_eq!(
            resolve_subject(
                Some(None),
                Some(PinSubject::WorkItem(2)),
                Some(PinSubject::Pr(1))
            ),
            None
        );
        assert_eq!(
            resolve_subject(None, Some(PinSubject::WorkItem(2)), Some(PinSubject::Pr(1))),
            Some(PinSubject::WorkItem(2))
        );
        assert_eq!(
            resolve_subject(None, None, Some(PinSubject::Pr(1))),
            Some(PinSubject::Pr(1))
        );
    }
}
