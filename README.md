# fileshare-linux

The Linux end of an FFSP benchmark rig: a laptop and an Android phone that bring up **LAN, USB
and Bluetooth at the same time**, stripe one file across all three, and report what each one
carried.

## What is where

| Crate | What it is |
|---|---|
| `crates/ffsp-linux` | The platform half: BlueZ, the USB host stack, TCP, mDNS, and the pairing store. No UI. |
| `crates/fileshare-linux` | The rig: which mediums to bring up, in what order, and a window to watch it in. |

Everything above the lane — framing, encryption, chunk scheduling, verification — is
[`ffsp-protocol`](../ffsp-protocol), which deliberately contains no transports at all.

## Running it

```sh
cargo run --release -- doctor          # what this machine can do
cargo run --release -- self-test       # the whole stack, no radio involved
cargo run --release -- receive         # wait for the phone, receive
cargo run --release -- send FILE       # wait for the phone, send
cargo run --release -- calibrate FILE  # each medium alone, then all at once
cargo run --release -- ui              # the window
```

`--mediums bluetooth,wifi,usb` picks which to use and in what order. Reports land in
`$XDG_DATA_HOME/crownos/fileshare/reports` as JSON and per-lane CSV.

**`self-test` proves the protocol, not the radios.** It runs two engines in one process over
in-memory lanes, so every "medium" runs at memory speed and its throughput numbers mean nothing
about Wi-Fi, USB or Bluetooth. What it does prove is that pairing, lane negotiation, striping,
verification and reporting all work — which separates "the protocol is wrong" from "the adapter
is wrong", otherwise the same red light.

## Why the mediums come up in that order

Bluetooth first. It needs no network configuration and no cable, only a pairing agent that
accepts — so it is the one medium that can be brought up with nothing already arranged. Once it
carries control frames, the other two are arranged over it: this end sends its IP address and
port, and asks the phone to become a USB accessory. That is what makes an unattended run
possible.

This end is always the session initiator, whichever medium connected first. Somebody has to be:
the initiator is the sole allocator of lane identifiers, and both ends derive a lane's
encryption key from its identifier, so two ends numbering independently would seal under
different keys the moment their hardware came up in a different order.

## USB needs a udev rule

Entering accessory mode makes the phone re-enumerate as `18d1:2d00`, which is not readable by a
normal user by default. `doctor` prints the rule. Note that `2d00` is accessory-only and
**kills adb**, while `2d01` keeps it — worth knowing when `logcat` disappears at exactly the
moment the USB lane comes up.

## What is not here

Wi-Fi Direct (the hardware supports it; a shared network is enough to benchmark), L2CAP CoC as a
faster Bluetooth path than RFCOMM, and multi-file transfers — one payload per transfer, because
the scheduler stripes one payload.
