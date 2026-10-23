//! [`frosted`]: frosted-glass overlays (palette, menus, dialogs) over a custom
//! background, using the zui fork's `Window::paint_backdrop_blur`.
//!
//! The full-window blur is baked into the background texture once (spec 0007
//! FR-4, no per-frame blur); this per-frame backdrop blur is limited to the
//! overlay's own bounds while it is open, and only while a background is
//! showing. Without one it is a pass-through, so overlays pay nothing.
//!
//! Adapted from zeron `crates/ui/src/frost.rs` (`Frosted`) at
//! d1010657d69c9730f8020edc36cc2649b136b6fa, MIT, Copyright (c) 2026 Wing.
//! See THIRD_PARTY_NOTICES.md. The whole subtree paints inside one scene
//! layer with the blur first, so a repaint elsewhere can never reorder the
//! card's own quads below the blur.

use adoken_theme::background::OVERLAY_BLUR;
use gpui::{
    AnyElement, App, Bounds, Corners, Element, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, Pixels, Window, px,
};

/// Frost `child` (an overlay card) when a background is showing.
/// `corner_radius` must match the card's rounding.
pub fn frosted(corner_radius: f32, child: impl IntoElement) -> Frosted {
    Frosted {
        corner_radius,
        child: child.into_any_element(),
    }
}

pub struct Frosted {
    corner_radius: f32,
    child: AnyElement,
}

impl Element for Frosted {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if super::is_showing(cx) {
            window.paint_layer(bounds, |window| {
                window.paint_backdrop_blur(
                    bounds,
                    Corners::all(px(self.corner_radius)),
                    px(OVERLAY_BLUR),
                );
                self.child.paint(window, cx);
            });
        } else {
            self.child.paint(window, cx);
        }
    }
}

impl IntoElement for Frosted {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}
