use std::fmt::Write as _;

use ffsp_linux::medium::{BluetoothMedium, UsbMedium, aoa};
use ffsp_linux::net;

/// Says what this machine can actually do, before a run blames the protocol for a missing radio.
pub(crate) async fn report() -> String {
    let mut out = String::new();
    let _ = writeln!(out, "FFSP rig readiness\n");

    match net::enumerate() {
        Ok(interfaces) if !interfaces.is_empty() => {
            for interface in &interfaces {
                let _ = writeln!(
                    out,
                    "  wi-fi        {} {} ({})",
                    tick(true),
                    interface.address,
                    interface.name
                );
            }
            let _ = writeln!(
                out,
                "               tunnels and loopbacks are skipped, so a VPN cannot be measured \
                 by mistake"
            );
        }
        _ => {
            let _ = writeln!(out, "  wi-fi        {} no routable interface", tick(false));
        }
    }

    let bluetooth = BluetoothMedium::is_available().await;
    let _ = writeln!(
        out,
        "  bluetooth    {} {}",
        tick(bluetooth),
        if bluetooth {
            "adapter present, pairing is automatic"
        } else {
            "no adapter, or bluetoothd is not running"
        }
    );

    let usb = UsbMedium::is_available();
    let _ = writeln!(
        out,
        "  usb          {} {}",
        tick(usb),
        if usb {
            "a device is attached; accessory mode is entered on demand"
        } else {
            "nothing attached"
        }
    );

    let accessory = aoa::attached()
        .map(|devices| {
            devices
                .iter()
                .any(|info| aoa::is_accessory(info.vendor_id(), info.product_id()))
        })
        .unwrap_or(false);
    if accessory {
        let _ = writeln!(out, "               already in accessory mode");
    }

    let _ = writeln!(
        out,
        "\n  aes hardware {} decides which cipher the two ends agree on",
        tick(ffsp_protocol::crypto::has_aes_hardware()),
    );

    if usb {
        let _ = writeln!(
            out,
            "\nIf the USB lane reports a permission error, install:\n\n{}",
            aoa::udev_rule()
        );
    }
    out
}

const fn tick(ok: bool) -> &'static str {
    if ok { "yes" } else { "no " }
}
