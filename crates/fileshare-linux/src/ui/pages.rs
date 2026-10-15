use blinc_icons::icons;
use crownuikit::widgets::{
    Segment, Tone, badge, button, callout, divider, progress, segmented_bar, status,
};
use xilem::WidgetView;
use xilem::masonry::properties::types::AsUnit;
use xilem::style::Style;
use xilem::view::{CrossAxisAlignment, FlexSpacer, flex_col, flex_row, label, prose, sized_box};

use crate::report::kbps;
use crate::ui::sparkline::sparkline;
use crate::ui::state::{AppState, Page, RigCommand, colour_of};

pub(crate) fn overview(state: &mut AppState) -> impl WidgetView<AppState> + use<> {
    let busy = state.busy;
    let chosen = state.chosen.clone();
    let readiness: Vec<_> = state.readiness.clone();
    let fraction = state.fraction();
    let running = state.progress.is_some();
    let status_line = state.status.clone();

    let mediums = flex_col(
        readiness
            .into_iter()
            .map(|(medium, ready)| {
                flex_row((
                    sized_box(status(medium.to_string()).tone(if ready {
                        Tone::Success
                    } else {
                        Tone::Danger
                    }))
                    .width(160.0.px()),
                    label(if ready {
                        "ready"
                    } else {
                        "unavailable — the run will use the others"
                    }),
                ))
                .gap(8.0.px())
            })
            .collect::<Vec<_>>(),
    )
    .gap(4.0.px())
    .cross_axis_alignment(CrossAxisAlignment::Start);

    let actions = flex_row((
        button("Receive", |state: &mut AppState| {
            state.command(RigCommand::Receive)
        })
        .variant(crownuikit::widgets::ButtonVariant::Primary)
        .leading_icon(icons::DOWNLOAD)
        .disabled(busy),
        button("Choose a file…", |state: &mut AppState| {
            if let Some(path) = rfd::FileDialog::new().pick_file() {
                state.chosen = Some(path);
            }
        })
        .leading_icon(icons::FOLDER_OPEN)
        .disabled(busy),
        button("Send", |state: &mut AppState| {
            if let Some(path) = state.chosen.clone() {
                state.command(RigCommand::Send(path));
            }
        })
        .leading_icon(icons::UPLOAD)
        .disabled(busy || chosen.is_none()),
        FlexSpacer::Flex(1.0),
        button("Self test", |state: &mut AppState| {
            state.command(RigCommand::SelfTest)
        })
        .variant(crownuikit::widgets::ButtonVariant::Ghost)
        .disabled(busy),
    ))
    .gap(8.0.px());

    let chosen_line = chosen
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "No file chosen".to_owned());

    flex_col((
        heading("Mediums"),
        mediums,
        divider(),
        heading("Transfer"),
        prose(chosen_line),
        actions,
        running.then(|| progress(fraction)),
        divider(),
        prose(status_line),
    ))
    .gap(12.0.px())
    .cross_axis_alignment(CrossAxisAlignment::Start)
}

pub(crate) fn benchmark(state: &mut AppState) -> impl WidgetView<AppState> + use<> {
    let ceiling = state.ceiling();
    let lanes = state.lanes.clone();
    let report = state.report.clone();

    if lanes.is_empty() && report.is_none() {
        return flex_col((
            heading("Nothing measured yet"),
            callout(
                Tone::Neutral,
                "Run a transfer, or the self test, to see what each medium carried.",
            )
            .body(
                "The self test runs the whole stack over in-memory lanes. It proves the \
                 protocol without a radio — and because every lane runs at memory speed, its \
                 throughput numbers say nothing about Wi-Fi, USB or Bluetooth.",
            )
            .view::<AppState, ()>(),
        ))
        .gap(12.0.px())
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .boxed();
    }

    let total: u64 = lanes.iter().map(|lane| lane.bytes).sum::<u64>().max(1);
    let share = segmented_bar(
        lanes
            .iter()
            .map(|lane| Segment::new(lane.bytes as f64, colour_of(lane.medium)))
            .collect::<Vec<_>>(),
    )
    .height(16.0);

    let traces = flex_col(
        lanes
            .iter()
            .map(|lane| {
                flex_col((
                    flex_row((
                        sized_box(badge(lane.medium.to_string(), colour_of(lane.medium)))
                            .width(140.0.px()),
                        label(kbps(lane.kbps)),
                        FlexSpacer::Flex(1.0),
                        label(format!("{:.1}%", lane.bytes as f64 / total as f64 * 100.0)),
                    ))
                    .gap(8.0.px()),
                    sparkline(lane.samples(), ceiling, colour_of(lane.medium)),
                ))
                .gap(4.0.px())
                .cross_axis_alignment(CrossAxisAlignment::Start)
            })
            .collect::<Vec<_>>(),
    )
    .gap(12.0.px())
    .cross_axis_alignment(CrossAxisAlignment::Start);

    let summary = report.map(|report| {
        flex_col((
            divider(),
            heading("Last run"),
            prose(crate::report::summarise(&report)),
        ))
        .gap(8.0.px())
        .cross_axis_alignment(CrossAxisAlignment::Start)
    });

    flex_col((
        heading("Share of the payload"),
        share,
        divider(),
        heading("Throughput per medium"),
        traces,
        summary,
    ))
    .gap(12.0.px())
    .cross_axis_alignment(CrossAxisAlignment::Start)
    .boxed()
}

pub(crate) fn page(state: &mut AppState) -> impl WidgetView<AppState> + use<> {
    match state.page {
        Page::Overview => overview(state).boxed(),
        Page::Benchmark => benchmark(state).boxed(),
    }
}

fn heading(text: &str) -> impl WidgetView<AppState> + use<> {
    sized_box(label(text.to_owned()).text_size(16.0)).padding(xilem::style::Padding::top(4.0))
}
