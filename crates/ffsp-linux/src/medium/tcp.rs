use std::net::SocketAddr;

use ffsp_protocol::medium::{LaneChannel, LaneProfile, framed_channel};
use ffsp_protocol::wire::LaneId;
use tokio::net::{TcpListener, TcpStream};

use crate::error::Result;

/// The port the rig listens on when nothing else is asked for.
pub const DEFAULT_PORT: u16 = 7423;

/// Accepts Wi-Fi lanes.
#[derive(Debug)]
pub struct TcpMedium {
    listener: TcpListener,
}

impl TcpMedium {
    pub async fn bind(address: SocketAddr) -> Result<Self> {
        Ok(Self {
            listener: TcpListener::bind(address).await?,
        })
    }

    pub fn local_addr(&self) -> Result<SocketAddr> {
        Ok(self.listener.local_addr()?)
    }

    /// Waits for a peer to connect, and turns the socket into a lane.
    pub async fn accept(&self) -> Result<(LaneChannel, SocketAddr)> {
        let (stream, peer) = self.listener.accept().await?;
        Ok((lane_from(stream)?, peer))
    }

    /// Reaches out to a peer instead of waiting for one.
    pub async fn connect(address: SocketAddr) -> Result<LaneChannel> {
        lane_from(TcpStream::connect(address).await?)
    }
}

fn lane_from(stream: TcpStream) -> Result<LaneChannel> {
    // Chunks are a megabyte; waiting 40 ms for Nagle to fill a segment adds latency to every
    // frame and buys nothing, because the frames are already large.
    stream.set_nodelay(true)?;
    Ok(framed_channel(
        LaneId::HANDSHAKE,
        LaneProfile::WIFI_INFRA,
        stream,
    ))
}
