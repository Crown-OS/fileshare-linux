use std::collections::VecDeque;
use std::path::PathBuf;

use crownui::kit::Theme;
use crownui::prelude::{Color, Cx, Runtime, Signal};
use ffsp_protocol::medium::Medium;
use ffsp_protocol::metrics::TransferReport;
use ffsp_protocol::transfer::Progress;
use ffsp_protocol::wire::LaneId;
use tokio::sync::mpsc::UnboundedSender;

/// How many samples the trace remembers: thirty seconds at ten a second.
pub(crate) const HISTORY: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Page {
    Overview,
    Benchmark,
}

/// What the window asks the rig to do.
#[derive(Debug, Clone)]
pub(crate) enum RigCommand {
    Receive,
    Send(PathBuf),
    SelfTest,
}

/// What the rig tells the window.
#[derive(Debug)]
pub(crate) enum RigEvent {
    Status(String),
    Readiness(Vec<(Medium, bool)>),
    Lane {
        lane: LaneId,
        medium: Medium,
        kbps: u32,
        bytes: u64,
    },
    Progress(Progress),
    Finished(Box<TransferReport>),
    Failed(String),
}

/// One medium as the window is currently seeing it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LaneView {
    pub(crate) lane: LaneId,
    pub(crate) medium: Medium,
    pub(crate) kbps: u32,
    pub(crate) bytes: u64,
    pub(crate) history: VecDeque<f64>,
}

impl LaneView {
    fn new(lane: LaneId, medium: Medium) -> Self {
        Self {
            lane,
            medium,
            kbps: 0,
            bytes: 0,
            history: VecDeque::with_capacity(HISTORY),
        }
    }

    fn observe(&mut self, kbps: u32, bytes: u64) {
        self.kbps = kbps;
        self.bytes = bytes;
        if self.history.len() == HISTORY {
            self.history.pop_front();
        }
        self.history.push_back(f64::from(kbps));
    }

    pub(crate) fn key(&self) -> LaneId {
        self.lane
    }
}

/// The window's state, as signals every view reads from.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Store {
    pub(crate) page: Signal<Page>,
    pub(crate) busy: Signal<bool>,
    pub(crate) status: Signal<String>,
    pub(crate) readiness: Signal<Vec<(Medium, bool)>>,
    pub(crate) lanes: Signal<Vec<LaneView>>,
    pub(crate) progress: Signal<Option<Progress>>,
    pub(crate) report: Signal<Option<TransferReport>>,
    pub(crate) chosen: Signal<Option<PathBuf>>,
    control: Signal<Option<UnboundedSender<RigCommand>>>,
}

impl Store {
    pub(crate) fn new(cx: &mut Cx) -> Self {
        Self {
            page: cx.signal(Page::Overview),
            busy: cx.signal(false),
            status: cx.signal("Idle. Bring the phone into range and press Receive.".to_owned()),
            readiness: cx.signal(Vec::new()),
            lanes: cx.signal(Vec::new()),
            progress: cx.signal(None),
            report: cx.signal(None),
            chosen: cx.signal(None),
            control: cx.signal(None),
        }
    }

    pub(crate) fn attach(self, runtime: &mut Runtime, sender: UnboundedSender<RigCommand>) {
        self.control.set(runtime, Some(sender));
    }

    /// Sends a command, or quietly does nothing if the worker has not started yet.
    pub(crate) fn command(self, runtime: &mut Runtime, command: RigCommand) {
        if self.busy.with_untracked(runtime, |busy| *busy) {
            return;
        }
        let sent = self.control.with_untracked(runtime, |control| {
            control
                .as_ref()
                .is_some_and(|control| control.send(command).is_ok())
        });
        if sent {
            self.busy.set(runtime, true);
            self.lanes.set(runtime, Vec::new());
            self.progress.set(runtime, None);
            self.report.set(runtime, None);
        }
    }

    pub(crate) fn apply(self, runtime: &mut Runtime, event: RigEvent) {
        match event {
            RigEvent::Status(status) => self.status.set(runtime, status),
            RigEvent::Readiness(readiness) => self.readiness.set(runtime, readiness),
            RigEvent::Lane {
                lane,
                medium,
                kbps,
                bytes,
            } => self.lanes.update(runtime, |lanes| {
                match lanes.iter_mut().find(|view| view.lane == lane) {
                    Some(view) => view.observe(kbps, bytes),
                    None => {
                        let mut view = LaneView::new(lane, medium);
                        view.observe(kbps, bytes);
                        lanes.push(view);
                    }
                }
            }),
            RigEvent::Progress(progress) => self.progress.set(runtime, Some(progress)),
            RigEvent::Finished(report) => {
                let status = format!(
                    "Done — {} across {} medium{}",
                    crate::report::kbps(report.aggregate_kbps()),
                    report.lanes.len(),
                    if report.lanes.len() == 1 { "" } else { "s" }
                );
                self.status.set(runtime, status);
                self.report.set(runtime, Some(*report));
                self.busy.set(runtime, false);
                self.page.set(runtime, Page::Benchmark);
            }
            RigEvent::Failed(reason) => {
                self.status.set(runtime, reason);
                self.busy.set(runtime, false);
            }
        }
    }
}

/// The scale every trace is drawn against, so two lanes can be compared by eye.
pub(crate) fn ceiling(lanes: &[LaneView]) -> f64 {
    lanes
        .iter()
        .flat_map(|lane| lane.history.iter().copied())
        .fold(1.0_f64, f64::max)
}

pub(crate) fn fraction(progress: Option<Progress>) -> f32 {
    progress
        .filter(|progress| progress.total_bytes > 0)
        .map_or(0.0, |progress| {
            (progress.delivered_bytes as f64 / progress.total_bytes as f64) as f32
        })
}

/// A stable colour per medium, taken from the palette so that it follows the theme.
pub(crate) fn colour_of(medium: Medium, theme: &Theme) -> Color {
    match medium {
        Medium::WifiInfra | Medium::WifiDirect => theme.accent.start,
        Medium::UsbAccessory => theme.status.success,
        Medium::BluetoothClassic | Medium::BluetoothLe => theme.status.warning,
        Medium::Loopback => theme.text.muted,
    }
}
