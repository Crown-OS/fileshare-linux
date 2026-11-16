//! A throughput trace, painted by hand.
//!
//! The trace uses a caller-supplied ceiling, not its own maximum. Thus two lanes side by side
//! use the same scale and you can compare them by eye.

use std::collections::VecDeque;

use crownui::ext::{DrawCommand, DrawList, Paint, Path, Stroke};
use crownui::kit::use_theme;
use crownui::prelude::{
    Color, CornerRadii, Point, Rect, Signal, Size, Styled, View, canvas, component,
};

use crate::ui::state::{LaneView, Store, ceiling, colour_of};

const HEIGHT: f32 = 44.0;
const WELL_RADIUS: f32 = 6.0;
const INSET: f32 = 3.0;
const LINE_WIDTH: f32 = 1.5;
const AREA_ALPHA: f32 = 0.18;

/// The trace of `lane`, scaled against the fastest sample of all lanes in `store`.
pub(crate) fn sparkline(store: Store, lane: Signal<LaneView>) -> impl View {
    component(move |cx| {
        let theme = use_theme(cx);
        canvas(move |runtime, size, list| {
            let ceiling = store.lanes.with(runtime, |lanes| ceiling(lanes));
            let medium = lane.with(runtime, |lane| lane.medium);
            let (track, colour) = theme.with(runtime, |theme| {
                (theme.control.track, colour_of(medium, theme))
            });
            list.fill(
                Rect::new(Point::ZERO, size),
                CornerRadii::uniform(WELL_RADIUS),
                track,
            );
            lane.with(runtime, |lane| {
                paint_trace(list, size, &lane.history, ceiling, colour);
            });
        })
        .w_full()
        .h(HEIGHT)
    })
}

fn paint_trace(
    list: &mut DrawList,
    size: Size,
    samples: &VecDeque<f64>,
    ceiling: f64,
    colour: Color,
) {
    let count = samples.len();
    if count < 2 || size.width <= 0.0 || ceiling <= 0.0 {
        return;
    }
    let usable = (size.height - INSET * 2.0).max(1.0);
    let step = size.width / (count - 1) as f32;
    let mut points = samples.iter().enumerate().map(|(index, value)| {
        let fraction = (value / ceiling).clamp(0.0, 1.0) as f32;
        Point::new(index as f32 * step, INSET + usable * (1.0 - fraction))
    });

    let Some(first) = points.next() else {
        return;
    };
    let mut line = Path::new().move_to(first);
    let mut area = Path::new()
        .move_to(Point::new(0.0, size.height))
        .line_to(first);
    for point in points {
        line = line.line_to(point);
        area = area.line_to(point);
    }
    area = area.line_to(Point::new(size.width, size.height)).close();

    list.push(DrawCommand::FillPath(
        area,
        Paint::solid(colour.with_alpha_factor(AREA_ALPHA)),
    ));
    list.push(DrawCommand::StrokePath(
        line,
        Stroke::new(LINE_WIDTH),
        Paint::solid(colour),
    ));
}
