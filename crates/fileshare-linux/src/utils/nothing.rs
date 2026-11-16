use crownui::prelude::{Styled, View, vstack};

/// A view that takes no space, for the empty branch of `show`.
pub(crate) fn nothing() -> impl View {
    vstack(()).display_none()
}
