use std::net::{IpAddr, Ipv4Addr};

use crate::error::{LinuxError, Result};

/// Interface name prefixes that are never the way to a phone in the same room.
///
/// A tunnel will happily carry the traffic and report a plausible address, which is exactly the
/// failure that makes a Wi-Fi benchmark measure a VPN.
const TUNNELS: [&str; 6] = ["lo", "tailscale", "tun", "tap", "wg", "docker"];

/// A local address worth advertising on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalInterface {
    pub name: String,
    pub address: Ipv4Addr,
    pub is_wireless: bool,
}

/// Picks the interface a peer on the same network can actually reach.
///
/// Wireless first, because that is the medium being measured; a wired interface is still better
/// than a tunnel.
pub fn preferred_interface() -> Result<LocalInterface> {
    let mut candidates = enumerate()?;
    candidates.sort_by_key(|interface| (!interface.is_wireless, interface.name.clone()));
    candidates
        .into_iter()
        .next()
        .ok_or(LinuxError::NoNetworkInterface)
}

/// Every routable IPv4 interface that is not a tunnel.
pub fn enumerate() -> Result<Vec<LocalInterface>> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir("/sys/class/net")? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if TUNNELS.iter().any(|prefix| name.starts_with(prefix)) {
            continue;
        }
        if std::fs::read_to_string(entry.path().join("operstate"))
            .map(|state| state.trim() != "up")
            .unwrap_or(true)
        {
            continue;
        }
        let Some(address) = address_of(&name) else {
            continue;
        };
        found.push(LocalInterface {
            is_wireless: entry.path().join("wireless").exists(),
            name,
            address,
        });
    }
    Ok(found)
}

/// Reads an interface's IPv4 address by asking the kernel for the route to it.
///
/// Parsing `/proc/net/route` would give the same answer with more code; binding a scratch socket
/// makes the kernel do the routing decision it would make for real traffic.
fn address_of(name: &str) -> Option<Ipv4Addr> {
    use std::os::unix::io::AsRawFd;

    let socket = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    bind_to_device(socket.as_raw_fd(), name).ok()?;
    // Any routable address will do; nothing is sent.
    socket.connect((Ipv4Addr::new(203, 0, 113, 1), 9)).ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(address) if !address.is_unspecified() => Some(address),
        _ => None,
    }
}

fn bind_to_device(fd: std::os::unix::io::RawFd, name: &str) -> std::io::Result<()> {
    // SO_BINDTODEVICE is the only way to ask "what address would this interface use", and it
    // needs the raw option because the standard library does not expose it.
    const SOL_SOCKET: i32 = 1;
    const SO_BINDTODEVICE: i32 = 25;

    let mut value = [0u8; 16];
    let bytes = name.as_bytes();
    let len = bytes.len().min(value.len() - 1);
    value
        .get_mut(..len)
        .ok_or_else(|| std::io::Error::other("interface name too long"))?
        .copy_from_slice(bytes.get(..len).unwrap_or_default());

    // SAFETY: `value` outlives the call and its length is passed alongside it.
    let outcome = unsafe {
        setsockopt(
            fd,
            SOL_SOCKET,
            SO_BINDTODEVICE,
            value.as_ptr().cast(),
            value.len() as u32,
        )
    };
    if outcome == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

unsafe extern "C" {
    #[link_name = "setsockopt"]
    fn setsockopt(
        fd: std::os::unix::io::RawFd,
        level: i32,
        name: i32,
        value: *const core::ffi::c_void,
        len: u32,
    ) -> i32;
}
