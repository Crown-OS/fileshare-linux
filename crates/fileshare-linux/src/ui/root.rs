use crownui::kit::{
    INTER_FAMILY, icons, provide_configured_theme, sidebar_item, sidebar_with_header,
    title_bar_spacer, use_theme, window_controls,
};
use crownui::prelude::{Listeners, Styled, View, bind, component};
use tokio::runtime::Handle;

use crate::ui::pages::page;
use crate::ui::state::{Page, Store};
use crate::ui::worker::connect_rig;

const CONTENT_PADDING: f32 = 20.0;

pub(crate) fn root(rig_runtime: Handle) -> impl View {
    component(move |cx| {
        provide_configured_theme(cx);
        let store = Store::new(cx);
        connect_rig(cx, store, &rig_runtime);
        shell(store)
    })
}

pub(crate) fn shell(store: Store) -> impl View {
    component(move |cx| {
        let theme = use_theme(cx);
        sidebar_with_header(
            (
                page_item(store, "Overview", icons::HOUSE, Page::Overview),
                page_item(store, "Benchmark", icons::CHART_LINE, Page::Benchmark),
            ),
            (title_bar_spacer(), window_controls()),
            page(store).p(CONTENT_PADDING),
        )
        .bg(bind(move |runtime| {
            theme.with(runtime, |theme| theme.surface.bg)
        }))
        .font_family(INTER_FAMILY)
    })
}

fn page_item(store: Store, label: &'static str, icon: &'static str, target: Page) -> impl View {
    sidebar_item(
        label,
        icon,
        bind(move |runtime| store.page.get(runtime) == target),
    )
    .on_click(move |cx, _| store.page.set(cx, target))
}
