use crownui::kit::{
    BadgeKind, CalloutOptions, Segment, SegmentedBarOptions, Tone, badge, callout, divider,
    segmented_bar, use_theme,
};
use crownui::prelude::{
    Cx, Element, Signal, Styled, View, bind, component, for_each, hstack, keyed, show, spacer,
    vstack,
};

use ffsp_protocol::medium::Medium;

use crate::report::{kbps, summarise};
use crate::ui::sparkline::sparkline;
use crate::ui::state::{LaneView, Store, colour_of};
use crate::ui::text::{body, heading};
use crate::utils::nothing::nothing;

const SHARE_BAR_HEIGHT: f32 = 16.0;
const MEDIUM_COLUMN_WIDTH: f32 = 140.0;

pub(crate) fn benchmark(store: Store) -> impl View {
    keyed(
        move |runtime| {
            store.lanes.with(runtime, Vec::is_empty) && store.report.with(runtime, Option::is_none)
        },
        move |_, nothing_measured| {
            if nothing_measured {
                empty_state().boxed()
            } else {
                measurements(store).boxed()
            }
        },
    )
}

fn empty_state() -> impl View {
    vstack((
        heading("Nothing measured yet"),
        callout(
            Tone::Neutral,
            "Run a transfer, or the self test, to see what each medium carried.",
        )
        .body(
            "The self test runs the whole stack over in-memory lanes. It proves the \
             protocol without a radio — and because every lane runs at memory speed, its \
             throughput numbers say nothing about Wi-Fi, USB or Bluetooth.",
        ),
    ))
    .gap(12.0)
}

fn measurements(store: Store) -> impl View {
    component(move |cx| {
        let theme = use_theme(cx);
        let share = segmented_bar([])
            .segments_from(move |runtime| {
                let theme = theme.get(runtime);
                store.lanes.with(runtime, |lanes| {
                    lanes
                        .iter()
                        .map(|lane| Segment::new(lane.bytes as f32, colour_of(lane.medium, &theme)))
                        .collect()
                })
            })
            .h(SHARE_BAR_HEIGHT);
        let traces = for_each(store.lanes, LaneView::key, move |_, lane| {
            lane_trace(store, lane)
        })
        .flex_col()
        .gap(12.0);
        let summary = show(
            bind(move |runtime| store.report.with(runtime, Option::is_some)),
            move || {
                let summary = bind(move |runtime| {
                    store
                        .report
                        .with(runtime, |report| report.as_ref().map(summarise))
                        .unwrap_or_default()
                });
                vstack((divider(), heading("Last run"), body(summary))).gap(8.0)
            },
            nothing,
        );
        vstack((
            heading("Share of the payload"),
            share,
            divider(),
            heading("Throughput per medium"),
            traces,
            summary,
        ))
        .gap(12.0)
    })
}

fn lane_trace(store: Store, lane: Signal<LaneView>) -> impl View {
    component(move |cx| {
        let medium = lane.with_untracked(cx, |lane| lane.medium);
        let throughput = bind(move |runtime| lane.with(runtime, |lane| kbps(lane.kbps)));
        let share = bind(move |runtime| {
            let total = store
                .lanes
                .with(runtime, |lanes| {
                    lanes.iter().map(|lane| lane.bytes).sum::<u64>()
                })
                .max(1);
            lane.with(runtime, |lane| {
                format!("{:.1}%", lane.bytes as f64 / total as f64 * 100.0)
            })
        });
        vstack((
            hstack((
                hstack((medium_badge(cx, medium),)).w(MEDIUM_COLUMN_WIDTH),
                body(throughput),
                spacer(),
                body(share),
            ))
            .gap(8.0)
            .items_center(),
            sparkline(store, lane),
        ))
        .gap(4.0)
    })
}

fn medium_badge(cx: &mut Cx, medium: Medium) -> Element<BadgeKind> {
    let theme = use_theme(cx);
    badge(
        medium.to_string(),
        bind(move |runtime| theme.with(runtime, |theme| colour_of(medium, theme))),
    )
}
