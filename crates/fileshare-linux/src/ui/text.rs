use crownui::kit::{TEXT_SIZE, use_theme};
use crownui::prelude::{IntoProp, Styled, View, bind, component, text};

const HEADING_SIZE: f32 = 16.0;

pub(crate) fn heading(content: &'static str) -> impl View {
    component(move |cx| {
        let theme = use_theme(cx);
        text(content)
            .text_size(HEADING_SIZE)
            .font_semibold()
            .pt(4.0)
            .text_color(bind(move |runtime| {
                theme.with(runtime, |theme| theme.text.primary)
            }))
    })
}

pub(crate) fn body(content: impl IntoProp<String> + 'static) -> impl View {
    component(move |cx| {
        let theme = use_theme(cx);
        text(content)
            .text_size(TEXT_SIZE)
            .text_color(bind(move |runtime| {
                theme.with(runtime, |theme| theme.text.body)
            }))
    })
}
