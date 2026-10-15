#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::aoa;

/// Where the Android app keeps the filter the accessory strings have to match.
const FILTER: &str = "../../../fileshare/app/src/main/res/xml/usb_accessory_filter.xml";

fn attribute<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("{name}=\"");
    let start = xml.find(&needle)? + needle.len();
    let rest = xml.get(start..)?;
    let end = rest.find('"')?;
    rest.get(..end)
}

/// The contract `ffsp-protocol`'s `accessory.rs` says should be checked, and until now was not.
///
/// A mismatch produces no intent and no error on either side: the phone re-enumerates in
/// accessory mode, no app is launched, and nothing anywhere explains why. Parsing the Android
/// resource here is the only place the two definitions ever meet.
#[test]
fn the_android_accessory_filter_matches_the_strings_this_host_sends() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(FILTER);
    let Ok(xml) = std::fs::read_to_string(&path) else {
        // The Android project is a sibling checkout, not a dependency; skip rather than fail
        // when only the Linux side is present.
        eprintln!("skipping: {} is not here", path.display());
        return;
    };

    assert_eq!(
        attribute(&xml, "manufacturer"),
        Some(ffsp_protocol::medium::AOA_MANUFACTURER),
        "the manufacturer string differs from the one this host sends"
    );
    assert_eq!(
        attribute(&xml, "model"),
        Some(ffsp_protocol::medium::AOA_MODEL),
        "the model string differs from the one this host sends"
    );
    assert_eq!(
        attribute(&xml, "version"),
        Some(ffsp_protocol::medium::AOA_VERSION),
        "the version string differs from the one this host sends"
    );
}

#[test]
fn an_accessory_is_recognised_with_or_without_the_debug_bridge() {
    assert!(aoa::is_accessory(
        aoa::ACCESSORY_VENDOR,
        aoa::ACCESSORY_PRODUCT
    ));
    assert!(aoa::is_accessory(
        aoa::ACCESSORY_VENDOR,
        aoa::ACCESSORY_PRODUCT_WITH_ADB
    ));
    assert!(!aoa::is_accessory(0x1234, aoa::ACCESSORY_PRODUCT));
}

/// A permission failure on USB is the most likely first-run problem, and the rule that fixes it
/// has to be printed rather than described.
#[test]
fn the_udev_rule_names_both_accessory_identities() {
    let rule = aoa::udev_rule();
    assert!(rule.contains("18d1"));
    assert!(rule.contains("2d00"));
    assert!(rule.contains("2d01"));
    assert!(rule.contains("udevadm control --reload-rules"));
}
