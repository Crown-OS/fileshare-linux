use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// Gives a transport handle a name it can be printed by.
///
/// [`LaneSender`](ffsp_protocol::medium::LaneSender) and its reading counterpart are `Debug`, so
/// a lane can always say what it is in a diagnostic. Not every operating system handle obliges —
/// a USB endpoint has nothing meaningful to print — and requiring one would put the burden on
/// the wrong side.
pub struct Named<T> {
    inner: T,
    name: &'static str,
}

impl<T> Named<T> {
    pub const fn new(name: &'static str, inner: T) -> Self {
        Self { inner, name }
    }
}

impl<T> core::fmt::Debug for Named<T> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(self.name)
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for Named<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_read(context, buffer)
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for Named<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(context, buffer)
    }

    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(context)
    }

    fn poll_shutdown(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(context)
    }
}
