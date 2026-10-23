//! Scrolling inside a scrolling host.
//!
//! gpui scrolls every `overflow_*_scroll` element under the pointer on each
//! wheel or trackpad event, so a list nested in a scrolling page (the theme
//! grid and the code font list in Settings › Appearance) dragged the page
//! along with it. [`ContainScroll::contain_scroll`] keeps the gesture in the
//! nested container, like CSS `overscroll-behavior: contain`.

use gpui::InteractiveElement;

pub trait ContainScroll: InteractiveElement + Sized {
    /// Stop wheel and trackpad scrolling over this element from reaching the
    /// scroll containers behind it. The element's own scroll listener still
    /// runs: gpui registers it after the element's wheel listeners, and the
    /// bubble phase calls them in reverse.
    fn contain_scroll(self) -> Self {
        self.on_scroll_wheel(|_, _, cx| cx.stop_propagation())
    }
}

impl<T: InteractiveElement> ContainScroll for T {}
