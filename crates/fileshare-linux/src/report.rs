use std::path::PathBuf;

use anyhow::{Context, Result};
use ffsp_protocol::metrics::{CalibrationReport, TransferReport};

/// Where a run's artefacts go, one directory per run.
#[derive(Debug, Clone)]
pub(crate) struct ReportDirectory {
    root: PathBuf,
}

impl ReportDirectory {
    pub(crate) fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub(crate) fn default_root() -> PathBuf {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
            })
            .unwrap_or_else(|| PathBuf::from("."))
            .join("crownos/fileshare/reports")
    }

    /// Writes the JSON and the per-lane CSV, and returns where they went.
    pub(crate) fn write(&self, label: &str, report: &TransferReport) -> Result<PathBuf> {
        std::fs::create_dir_all(&self.root)
            .with_context(|| format!("creating {}", self.root.display()))?;
        let stem = format!("{}-{label}-{}", stamp(), report.role.as_str());
        let json = self.root.join(format!("{stem}.json"));
        let csv = self.root.join(format!("{stem}.csv"));
        std::fs::write(&json, report.to_json())
            .with_context(|| format!("writing {}", json.display()))?;
        std::fs::write(&csv, report.to_csv())
            .with_context(|| format!("writing {}", csv.display()))?;
        Ok(json)
    }

    pub(crate) fn write_calibration(&self, report: &CalibrationReport) -> Result<PathBuf> {
        std::fs::create_dir_all(&self.root)?;
        let path = self.root.join(format!("{}-calibration.json", stamp()));
        std::fs::write(&path, report.to_json())?;
        Ok(path)
    }
}

/// Seconds since the epoch, which sorts and needs no date library.
fn stamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

/// A short human summary, for the terminal rather than the file.
pub(crate) fn summarise(report: &TransferReport) -> String {
    let mut out = String::new();
    let megabytes = report.total_bytes / (1024 * 1024);
    let seconds = report.wall_micros as f64 / 1_000_000.0;
    out.push_str(&format!(
        "{} {} MiB in {seconds:.2}s — {} aggregate\n",
        report.role.as_str(),
        megabytes,
        kbps(report.aggregate_kbps()),
    ));
    out.push_str(&format!(
        "  best single medium {}, speedup {:.2}x\n",
        kbps(report.best_lane_kbps()),
        report.speedup_permille() as f64 / 1000.0,
    ));
    for lane in &report.lanes {
        out.push_str(&format!(
            "  {:<14} {:>6} chunks  {:>5.1}%  {:>10}  rtt p50 {:>7}  {}\n",
            lane.medium.to_string(),
            lane.carried_chunks,
            lane.share_permille(report.total_bytes) as f64 / 10.0,
            kbps(lane.mean_kbps),
            lane.rtt_p50_micros
                .map(|micros| format!("{:.2}ms", micros as f64 / 1000.0))
                .unwrap_or_else(|| "-".to_owned()),
            if lane.parked { "parked" } else { "" },
        ));
    }
    if report.buffer_starvations > 0 {
        out.push_str(&format!(
            "  the payload outran the lanes {} times\n",
            report.buffer_starvations
        ));
    }
    out
}

/// Kilobits per second, shown in the unit a person reads throughput in.
pub(crate) fn kbps(value: u32) -> String {
    let megabytes_per_second = f64::from(value) / 8_000.0;
    if megabytes_per_second >= 1.0 {
        format!("{megabytes_per_second:.1} MB/s")
    } else {
        format!("{:.0} kB/s", f64::from(value) / 8.0)
    }
}
