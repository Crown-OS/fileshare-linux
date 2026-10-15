//! The Linux end of the FFSP benchmark rig.
//!
//! Everything the protocol needs from this machine — the radios, the bus, the pairing store —
//! comes from `ffsp-linux`; everything above the lane comes from `ffsp-protocol`. What is here
//! is the arrangement: which mediums to bring up, in which order, and what to do once they are.

mod doctor;
mod report;
mod selftest;
mod session;
mod ui;

use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::{Parser, Subcommand};
use ffsp_protocol::medium::Medium;
use ffsp_protocol::metrics::{CalibrationReport, StandaloneRun, TransferReport};

use crate::report::ReportDirectory;
use crate::session::{Rig, RigOptions};

#[derive(Debug, Parser)]
#[command(
    name = "fileshare-linux",
    about = "Multi-medium file transfer, measured"
)]
struct Cli {
    /// Which mediums to bring up, in the order they should be brought up.
    #[arg(long, value_delimiter = ',', default_value = "bluetooth,wifi,usb")]
    mediums: Vec<MediumArg>,

    /// Where reports are written.
    #[arg(long)]
    reports: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Waits for the phone, then receives whatever it offers.
    Receive {
        #[arg(long, default_value = ".")]
        destination: PathBuf,
        #[arg(long, default_value_t = ffsp_linux::medium::tcp::DEFAULT_PORT)]
        port: u16,
    },
    /// Waits for the phone, then sends a file to it.
    Send {
        path: PathBuf,
        #[arg(long, default_value_t = ffsp_linux::medium::tcp::DEFAULT_PORT)]
        port: u16,
    },
    /// Runs the whole stack against itself, with no radio involved.
    SelfTest {
        /// Payload size in mebibytes.
        #[arg(long, default_value_t = 64)]
        size: u64,
        /// How many lanes to stripe across.
        #[arg(long, default_value_t = 3)]
        lanes: usize,
    },
    /// Sends the same payload over each medium alone, then over all of them at once.
    ///
    /// The combined figure on its own cannot be checked: only the standalone runs say whether
    /// the second and third medium were worth their setup cost.
    Calibrate {
        path: PathBuf,
        #[arg(long, default_value_t = ffsp_linux::medium::tcp::DEFAULT_PORT)]
        port: u16,
    },
    /// Brings the USB lane up on its own, and says exactly how it failed if it does.
    ///
    /// The USB path has more ways to go wrong than the others — permissions, accessory mode,
    /// a stale claim — and each reports itself as a bare errno somewhere in the middle of a
    /// transfer. Exercising it alone turns that into one legible answer.
    UsbProbe,
    /// Says what this machine can do.
    Doctor,
    /// Opens the window.
    Ui,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum MediumArg {
    Bluetooth,
    Wifi,
    Usb,
}

impl From<MediumArg> for Medium {
    fn from(value: MediumArg) -> Self {
        match value {
            MediumArg::Bluetooth => Self::BluetoothClassic,
            MediumArg::Wifi => Self::WifiInfra,
            MediumArg::Usb => Self::UsbAccessory,
        }
    }
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();
    if matches!(cli.command, Command::Ui) {
        return ui::run();
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(run(cli))
}

async fn run(cli: Cli) -> Result<()> {
    let reports = ReportDirectory::new(
        cli.reports
            .clone()
            .unwrap_or_else(ReportDirectory::default_root),
    );
    let mediums: Vec<Medium> = cli.mediums.iter().copied().map(Medium::from).collect();

    match cli.command {
        Command::Doctor => {
            print!("{}", doctor::report().await);
            Ok(())
        }
        Command::UsbProbe => {
            for device in ffsp_linux::medium::aoa::attached()? {
                println!(
                    "  {:04x}:{:04x}  accessory={}",
                    device.vendor_id(),
                    device.product_id(),
                    ffsp_linux::medium::aoa::is_accessory(
                        device.vendor_id(),
                        device.product_id()
                    ),
                );
            }
            match ffsp_linux::medium::UsbMedium::new("probe").open().await {
                Ok(channel) => {
                    println!("\nusb lane opened: {:?}", channel.profile().medium);
                    Ok(())
                }
                Err(error) => {
                    println!("\nusb lane failed: {error}");
                    println!("  debug: {error:?}");
                    Ok(())
                }
            }
        }
        Command::SelfTest { size, lanes } => {
            let bytes = size * 1024 * 1024;
            tracing::info!(mebibytes = size, lanes, "running the stack against itself");
            // One directory per run: two self tests sharing a scratch path clobber each
            // other's payload, and the symptom is a transfer that never finishes.
            let scratch = scratch_directory();
            let report = selftest::run(lanes, bytes, scratch.clone()).await?;
            let _ = std::fs::remove_dir_all(&scratch);
            print!("{}", report::summarise(&report));
            let path = reports.write("selftest", &report)?;
            tracing::info!(path = %path.display(), "report written");
            Ok(())
        }
        Command::Receive { destination, port } => {
            let mut rig = Rig::start(RigOptions {
                mediums,
                port,
                destination,
                ..RigOptions::default()
            })?;
            rig.bring_up().await?;
            let report = rig.receive().await?;
            print!("{}", report::summarise(&report));
            let path = reports.write("receive", &report)?;
            tracing::info!(path = %path.display(), "report written");
            Ok(())
        }
        Command::Send { path, port } => {
            let mut rig = Rig::start(RigOptions {
                mediums,
                port,
                destination: std::env::temp_dir().join("ffsp-inbox"),
                ..RigOptions::default()
            })?;
            let peer = rig.bring_up().await?;
            let report = rig.send(peer, path).await?;
            print!("{}", report::summarise(&report));
            let written = reports.write("send", &report)?;
            tracing::info!(path = %written.display(), "report written");
            Ok(())
        }
        Command::Calibrate { path, port } => {
            let calibration = calibrate(&mediums, &path, port, &reports).await?;
            print!("{}", summarise_calibration(&calibration));
            let written = reports.write_calibration(&calibration)?;
            tracing::info!(path = %written.display(), "calibration written");
            Ok(())
        }
        Command::Ui => Ok(()),
    }
}

/// One medium at a time, then all of them.
async fn calibrate(
    mediums: &[Medium],
    path: &Path,
    port: u16,
    reports: &ReportDirectory,
) -> Result<CalibrationReport> {
    let mut standalone = Vec::new();
    for medium in mediums {
        tracing::info!(%medium, "calibrating on its own — reconnect the phone when prompted");
        let report = one_run(&[*medium], path, port).await?;
        print!("{}", report::summarise(&report));
        reports.write(&format!("alone-{medium}"), &report)?;
        standalone.push(StandaloneRun {
            medium: *medium,
            bytes: report.total_bytes,
            wall_micros: report.wall_micros,
        });
    }

    tracing::info!("calibrating with every medium at once");
    let combined_report = one_run(mediums, path, port).await?;
    print!("{}", report::summarise(&combined_report));
    reports.write("combined", &combined_report)?;

    Ok(CalibrationReport {
        label: format!("{} bytes", combined_report.total_bytes).into(),
        standalone,
        combined: Some(StandaloneRun {
            medium: *mediums.first().unwrap_or(&Medium::WifiInfra),
            bytes: combined_report.total_bytes,
            wall_micros: combined_report.wall_micros,
        }),
    })
}

async fn one_run(mediums: &[Medium], path: &Path, port: u16) -> Result<TransferReport> {
    let mut rig = Rig::start(RigOptions {
        mediums: mediums.to_vec(),
        port,
        destination: std::env::temp_dir().join("ffsp-inbox"),
        ..RigOptions::default()
    })?;
    let peer = rig.bring_up().await?;
    rig.send(peer, path.to_path_buf()).await
}

fn summarise_calibration(calibration: &CalibrationReport) -> String {
    let mut out = String::from("\ncalibration\n");
    for run in &calibration.standalone {
        out.push_str(&format!(
            "  {:<14} alone      {}\n",
            run.medium.to_string(),
            report::kbps(run.kbps())
        ));
    }
    if let Some(combined) = calibration.combined {
        out.push_str(&format!(
            "  {:<14} together   {}\n",
            "all",
            report::kbps(combined.kbps())
        ));
    }
    match calibration.speedup_permille() {
        Some(speedup) => out.push_str(&format!(
            "  speedup over the best single medium: {:.2}x\n",
            speedup as f64 / 1000.0
        )),
        None => out.push_str("  no speedup could be computed\n"),
    }
    out
}

/// A scratch path unique to this process, so concurrent runs cannot collide.
fn scratch_directory() -> PathBuf {
    std::env::temp_dir().join(format!("ffsp-selftest-{}", std::process::id()))
}
