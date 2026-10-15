//! The window.

mod pages;
mod sparkline;
mod state;
mod worker;

use blinc_icons::icons;
use crownuikit::config::{load_appearance, theme, themed};
use crownuikit::layouts::sidebar::{sidebar, sidebar_item};
use crownuikit::util::inter_font;
use winit::dpi::LogicalSize;
use xilem::core::fork;
use xilem::masonry::properties::types::AsUnit;
use xilem::style::Style;
use xilem::view::{CrossAxisAlignment, flex_col, sized_box};
use xilem::{EventLoop, WidgetView, WindowOptions, Xilem};

use crate::ui::state::{AppState, Page};
use crate::ui::worker::rig_worker;

const INITIAL_SIZE: LogicalSize<f64> = LogicalSize::new(980.0, 700.0);
const MIN_SIZE: LogicalSize<f64> = LogicalSize::new(720.0, 520.0);

pub(crate) fn run() -> anyhow::Result<()> {
    // Before the first frame, so the window opens in the mode the user already chose.
    load_appearance();

    let window = WindowOptions::new("FileShare")
        .with_initial_inner_size(INITIAL_SIZE)
        .with_min_inner_size(MIN_SIZE);

    Xilem::new_simple(AppState::default(), root_view, window)
        .with_font(inter_font())
        .run_in(EventLoop::with_user_event())?;
    Ok(())
}

fn root_view(state: &mut AppState) -> impl WidgetView<AppState> + use<> {
    let page = state.page;
    let menus = flex_col((
        sidebar_item(
            "Overview",
            icons::HOUSE,
            page == Page::Overview,
            |state: &mut AppState| {
                state.page = Page::Overview;
            },
        ),
        sidebar_item(
            "Benchmark",
            icons::CHART_LINE,
            page == Page::Benchmark,
            |state: &mut AppState| state.page = Page::Benchmark,
        ),
    ))
    .gap(2.0.px())
    .cross_axis_alignment(CrossAxisAlignment::Start);

    let content = sized_box(pages::page(state)).padding(20.0);
    let root = sized_box(sidebar(menus, content))
        .expand()
        .background_color(theme().surface.bg);

    // The worker lives at the root, not inside a page: a worker rebuilt on navigation would
    // reconnect and start the rig again every time the sidebar is clicked.
    fork(themed(root), (rig_worker(),))
}
