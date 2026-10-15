//! A throughput trace, painted by hand.
//!
//! crownuikit has no chart, and it is right not to: a general chart widget would have to be
//! configurable in ways this needs none of. What this needs is one series, filling its box, in
//! a colour that follows the theme — which is thirty lines of painting and no configuration.
//!
//! Hand-painted rather than composed for the reason the kit gives for its other hand-painted
//! widgets: the geometry has to be exact. A flex row of bars would round each column's width
//! independently and the trace would drift against its own axis.

use xilem::core::{MessageContext, MessageResult, Mut, View, ViewMarker};
use xilem::masonry::accesskit::{Node, Role};
use xilem::masonry::core::{
    AccessCtx, BoxConstraints, ChildrenIds, LayoutCtx, NoAction, PaintCtx, PropertiesMut,
    PropertiesRef, RegisterCtx, Widget, WidgetMut,
};
use xilem::masonry::kurbo::{Affine, BezPath, Point, Rect, Size, Stroke};
use xilem::masonry::peniko::{Color, Fill};
use xilem::masonry::vello::Scene;
use xilem::{Pod, ViewCtx};

use crownuikit::config::{repaint_on_theme_change, theme, theme_epoch};

const DEFAULT_HEIGHT: f64 = 44.0;

/// One series of throughput samples, newest last.
#[derive(Debug)]
pub(crate) struct Sparkline {
    samples: Vec<f64>,
    ceiling: f64,
    colour: Color,
    height: f64,
}

impl Sparkline {
    const fn new(samples: Vec<f64>, ceiling: f64, colour: Color, height: f64) -> Self {
        Self {
            samples,
            ceiling,
            colour,
            height,
        }
    }

    fn set_samples(this: &mut WidgetMut<'_, Self>, samples: Vec<f64>, ceiling: f64) {
        this.widget.samples = samples;
        this.widget.ceiling = ceiling;
        this.ctx.request_paint_only();
    }

    fn set_colour(this: &mut WidgetMut<'_, Self>, colour: Color) {
        if this.widget.colour != colour {
            this.widget.colour = colour;
            this.ctx.request_paint_only();
        }
    }
}

impl Widget for Sparkline {
    type Action = NoAction;

    fn accepts_pointer_interaction(&self) -> bool {
        false
    }

    fn register_children(&mut self, _ctx: &mut RegisterCtx<'_>) {}

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx<'_>,
        _props: &mut PropertiesMut<'_>,
        constraints: &BoxConstraints,
    ) -> Size {
        Size::new(constraints.max().width, self.height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx<'_>, _props: &PropertiesRef<'_>, scene: &mut Scene) {
        let palette = theme();
        let size = ctx.size();
        let well = Rect::new(0.0, 0.0, size.width, size.height).to_rounded_rect(6.0);
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            palette.control.track,
            None,
            &well,
        );

        if self.samples.len() < 2 || size.width <= 0.0 || self.ceiling <= 0.0 {
            return;
        }

        // The trace is drawn against a caller-supplied ceiling rather than its own maximum, so
        // two lanes shown side by side are on the same scale and can be compared by eye.
        let inset = 3.0;
        let usable = (size.height - inset * 2.0).max(1.0);
        let step = size.width / (self.samples.len() - 1) as f64;
        let point_at = |index: usize, value: f64| {
            let fraction = (value / self.ceiling).clamp(0.0, 1.0);
            Point::new(index as f64 * step, inset + usable * (1.0 - fraction))
        };

        let mut line = BezPath::new();
        let mut area = BezPath::new();
        area.move_to(Point::new(0.0, size.height));
        for (index, value) in self.samples.iter().enumerate() {
            let point = point_at(index, *value);
            if index == 0 {
                line.move_to(point);
            } else {
                line.line_to(point);
            }
            area.line_to(point);
        }
        area.line_to(Point::new(size.width, size.height));
        area.close_path();

        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            self.colour.with_alpha(0.18),
            None,
            &area,
        );
        scene.stroke(
            &Stroke::new(1.5),
            Affine::IDENTITY,
            self.colour,
            None,
            &line,
        );
    }

    fn accessibility_role(&self) -> Role {
        Role::Group
    }

    fn accessibility(
        &mut self,
        _ctx: &mut AccessCtx<'_>,
        _props: &PropertiesRef<'_>,
        _node: &mut Node,
    ) {
    }

    fn children_ids(&self) -> ChildrenIds {
        ChildrenIds::new()
    }
}

/// A throughput trace scaled against `ceiling`.
pub(crate) fn sparkline(samples: Vec<f64>, ceiling: f64, colour: Color) -> SparklineView {
    SparklineView {
        samples,
        ceiling,
        colour,
        height: DEFAULT_HEIGHT,
        theme_epoch: theme_epoch(),
    }
}

#[must_use]
#[derive(Debug)]
pub(crate) struct SparklineView {
    samples: Vec<f64>,
    ceiling: f64,
    colour: Color,
    height: f64,
    theme_epoch: u64,
}

impl ViewMarker for SparklineView {}
impl<State, Action> View<State, Action, ViewCtx> for SparklineView
where
    State: 'static,
    Action: 'static,
{
    type Element = Pod<Sparkline>;
    type ViewState = ();

    fn build(&self, ctx: &mut ViewCtx, _: &mut State) -> (Self::Element, Self::ViewState) {
        (
            ctx.create_pod(Sparkline::new(
                self.samples.clone(),
                self.ceiling,
                self.colour,
                self.height,
            )),
            (),
        )
    }

    fn rebuild(
        &self,
        prev: &Self,
        (): &mut Self::ViewState,
        _: &mut ViewCtx,
        mut element: Mut<'_, Self::Element>,
        _: &mut State,
    ) {
        // The trace samples the palette while painting, so a theme change has to reach it even
        // when nothing about the data moved.
        repaint_on_theme_change(prev.theme_epoch, self.theme_epoch, &mut element);
        if prev.samples != self.samples || prev.ceiling != self.ceiling {
            Sparkline::set_samples(&mut element, self.samples.clone(), self.ceiling);
        }
        if prev.colour != self.colour {
            Sparkline::set_colour(&mut element, self.colour);
        }
    }

    fn teardown(
        &self,
        (): &mut Self::ViewState,
        ctx: &mut ViewCtx,
        element: Mut<'_, Self::Element>,
    ) {
        ctx.teardown_leaf(element);
    }

    fn message(
        &self,
        (): &mut Self::ViewState,
        _message: &mut MessageContext,
        _element: Mut<'_, Self::Element>,
        _state: &mut State,
    ) -> MessageResult<Action> {
        MessageResult::Nop
    }
}
