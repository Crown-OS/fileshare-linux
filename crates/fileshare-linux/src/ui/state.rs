use std::collections::VecDeque;
use std::path::PathBuf;

use ffsp_protocol::medium::Medium;
use ffsp_protocol::metrics::TransferReport;
use ffsp_protocol::transfer::Progress;
use ffsp_protocol::wire::LaneId;
use xilem::masonry::peniko::Color;
use xilem::tokio::sync::mpsc::UnboundedSender;

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
#[derive(Debug, Clone)]
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

    pub(crate) fn samples(&self) -> Vec<f64> {
        self.history.iter().copied().collect()
    }
}

#[derive(Debug)]
pub(crate) struct AppState {
    pub(crate) page: Page,
    pub(crate) busy: bool,
    pub(crate) status: String,
    pub(crate) readiness: Vec<(Medium, bool)>,
    pub(crate) lanes: Vec<LaneView>,
    pub(crate) progress: Option<Progress>,
    pub(crate) report: Option<TransferReport>,
    pub(crate) chosen: Option<PathBuf>,
    control: Option<UnboundedSender<RigCommand>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            page: Page::Overview,
            busy: false,
            status: "Idle. Bring the phone into range and press Receive.".to_owned(),
            readiness: Vec::new(),
            lanes: Vec::new(),
            progress: None,
            report: None,
            chosen: None,
            control: None,
        }
    }
}

impl AppState {
    pub(crate) fn attach(&mut self, sender: UnboundedSender<RigCommand>) {
        self.control = Some(sender);
    }

    /// Sends a command, or quietly does nothing if the worker has not started yet.
    pub(crate) fn command(&mut self, command: RigCommand) {
        if self.busy {
            return;
        }
        if let Some(control) = &self.control
            && control.send(command).is_ok()
        {
            self.busy = true;
            self.lanes.clear();
            self.progress = None;
            self.report = None;
        }
    }

    pub(crate) fn apply(&mut self, event: RigEvent) {
        match event {
            RigEvent::Status(status) => self.status = status,
            RigEvent::Readiness(readiness) => self.readiness = readiness,
            RigEvent::Lane {
                lane,
                medium,
                kbps,
                bytes,
            } => {
                let view = match self.lanes.iter_mut().find(|view| view.lane == lane) {
                    Some(view) => view,
                    None => {
                        self.lanes.push(LaneView::new(lane, medium));
                        match self.lanes.last_mut() {
                            Some(view) => view,
                            None => return,
                        }
                    }
                };
                view.observe(kbps, bytes);
            }
            RigEvent::Progress(progress) => self.progress = Some(progress),
            RigEvent::Finished(report) => {
                self.status = format!(
                    "Done — {} across {} medium{}",
                    crate::report::kbps(report.aggregate_kbps()),
                    report.lanes.len(),
                    if report.lanes.len() == 1 { "" } else { "s" }
                );
                self.report = Some(*report);
                self.busy = false;
                self.page = Page::Benchmark;
            }
            RigEvent::Failed(reason) => {
                self.status = reason;
                self.busy = false;
            }
        }
    }

    /// The scale every trace is drawn against, so two lanes can be compared by eye.
    pub(crate) fn ceiling(&self) -> f64 {
        self.lanes
            .iter()
            .flat_map(|lane| lane.history.iter().copied())
            .fold(1.0_f64, f64::max)
    }

    pub(crate) fn fraction(&self) -> f64 {
        self.progress
            .map(|progress| {
                if progress.total_bytes == 0 {
                    0.0
                } else {
                    progress.delivered_bytes as f64 / progress.total_bytes as f64
                }
            })
            .unwrap_or(0.0)
    }
}

/// A stable colour per medium, taken from the palette rather than invented.
///
/// The kit's one rule is that no widget names a colour, and a benchmark legend is no exception:
/// these have to follow the theme like everything else.
pub(crate) fn colour_of(medium: Medium) -> Color {
    let palette = crownuikit::config::theme();
    match medium {
        Medium::WifiInfra | Medium::WifiDirect => palette.accent.start,
        Medium::UsbAccessory => palette.status.success,
        Medium::BluetoothClassic | Medium::BluetoothLe => palette.status.warning,
        Medium::Loopback => palette.text.muted,
    }
}
