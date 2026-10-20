//! A passive HTTP/2 frame scanner on the plaintext connection IO that
//! measures PING round trips (hyper keeps its PING handling internal).

use std::collections::VecDeque;
use std::io;
use std::pin::Pin;
use std::sync::{Mutex, PoisonError};
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::time::Instant;

/// The client connection preface every HTTP/2 client sends first.
pub(crate) const PREFACE: &[u8; 24] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
/// The PING frame type.
const PING: u8 = 0x6;
/// The ACK flag of a PING frame.
const ACK: u8 = 0x1;
/// How many unanswered PINGs are remembered (the oldest is evicted).
const MAX_OUTSTANDING: usize = 4;

/// Where the scanner is inside the byte stream.
#[derive(Debug, Clone, Copy)]
enum State {
    /// Skipping the client preface: bytes left.
    Preface(usize),
    /// Reading a 9-byte frame header.
    Header { buf: [u8; 9], filled: usize },
    /// Inside a payload: bytes left, and the PING being captured.
    Payload { left: usize, ping: Option<Ping> },
}

#[derive(Debug, Clone, Copy)]
struct Ping {
    ack: bool,
    payload: [u8; 8],
    filled: usize,
}

/// Finds PING frames in one direction of an HTTP/2 byte stream, whatever
/// the read/write boundaries are.
#[derive(Debug, Clone)]
pub(crate) struct FrameScanner {
    state: State,
}

impl FrameScanner {
    /// A scanner for the client-to-server direction (starts with the
    /// preface).
    pub(crate) fn outbound() -> Self {
        Self {
            state: State::Preface(PREFACE.len()),
        }
    }

    /// A scanner for the server-to-client direction.
    pub(crate) fn inbound() -> Self {
        Self {
            state: State::Header {
                buf: [0; 9],
                filled: 0,
            },
        }
    }

    /// Scans `bytes`, calling `on_ping(ack, payload)` for every complete
    /// PING frame.
    pub(crate) fn feed(&mut self, mut bytes: &[u8], mut on_ping: impl FnMut(bool, [u8; 8])) {
        while !bytes.is_empty() {
            match &mut self.state {
                State::Preface(left) => {
                    let n = (*left).min(bytes.len());
                    *left -= n;
                    bytes = &bytes[n..];
                    if *left == 0 {
                        self.state = State::Header {
                            buf: [0; 9],
                            filled: 0,
                        };
                    }
                }
                State::Header { buf, filled } => {
                    let n = (9 - *filled).min(bytes.len());
                    buf[*filled..*filled + n].copy_from_slice(&bytes[..n]);
                    *filled += n;
                    bytes = &bytes[n..];
                    if *filled == 9 {
                        let len = usize::from(buf[0]) << 16
                            | usize::from(buf[1]) << 8
                            | usize::from(buf[2]);
                        let ping = (buf[3] == PING && len == 8).then_some(Ping {
                            ack: buf[4] & ACK != 0,
                            payload: [0; 8],
                            filled: 0,
                        });
                        self.state = State::Payload { left: len, ping };
                        self.finish_payload(&mut on_ping);
                    }
                }
                State::Payload { left, ping } => {
                    let n = (*left).min(bytes.len());
                    if let Some(ping) = ping {
                        ping.payload[ping.filled..ping.filled + n].copy_from_slice(&bytes[..n]);
                        ping.filled += n;
                    }
                    *left -= n;
                    bytes = &bytes[n..];
                    self.finish_payload(&mut on_ping);
                }
            }
        }
    }

    /// Ends a payload whose last byte was read (or that is empty).
    fn finish_payload(&mut self, on_ping: &mut impl FnMut(bool, [u8; 8])) {
        if let State::Payload { left: 0, ping } = self.state {
            if let Some(ping) = ping {
                on_ping(ping.ack, ping.payload);
            }
            self.state = State::Header {
                buf: [0; 9],
                filled: 0,
            };
        }
    }
}

/// Matches PINGs to their ACKs and reports each round trip.
pub(crate) struct PingTracker {
    outstanding: Mutex<VecDeque<([u8; 8], Instant)>>,
    on_rtt: Box<dyn Fn(Duration) + Send + Sync>,
}

impl PingTracker {
    pub(crate) fn new(on_rtt: impl Fn(Duration) + Send + Sync + 'static) -> Self {
        Self {
            outstanding: Mutex::new(VecDeque::new()),
            on_rtt: Box::new(on_rtt),
        }
    }

    /// A PING (not an ACK) left the client.
    pub(crate) fn sent(&self, payload: [u8; 8]) {
        let mut outstanding = self
            .outstanding
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if outstanding.len() == MAX_OUTSTANDING {
            outstanding.pop_front();
        }
        outstanding.push_back((payload, Instant::now()));
    }

    /// A PING ACK arrived; an unknown payload is ignored.
    pub(crate) fn acked(&self, payload: [u8; 8]) {
        let sent = {
            let mut outstanding = self
                .outstanding
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let Some(index) = outstanding.iter().position(|(p, _)| *p == payload) else {
                return;
            };
            outstanding.remove(index).map(|(_, at)| at)
        };
        if let Some(at) = sent {
            (self.on_rtt)(at.elapsed());
        }
    }

    /// How many PINGs wait for their ACK.
    #[cfg(test)]
    pub(crate) fn outstanding(&self) -> usize {
        self.outstanding
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }
}

/// Connection IO that feeds both directions to a [`PingTracker`].
pub(crate) struct Sniff<T> {
    io: T,
    outbound: FrameScanner,
    inbound: FrameScanner,
    tracker: std::sync::Arc<PingTracker>,
}

impl<T> Sniff<T> {
    pub(crate) fn new(io: T, tracker: std::sync::Arc<PingTracker>) -> Self {
        Self {
            io,
            outbound: FrameScanner::outbound(),
            inbound: FrameScanner::inbound(),
            tracker,
        }
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for Sniff<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let before = buf.filled().len();
        let polled = Pin::new(&mut this.io).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = polled {
            let tracker = &this.tracker;
            this.inbound.feed(&buf.filled()[before..], |ack, payload| {
                if ack {
                    tracker.acked(payload);
                }
            });
        }
        polled
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for Sniff<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let polled = Pin::new(&mut this.io).poll_write(cx, buf);
        if let Poll::Ready(Ok(n)) = polled {
            let tracker = &this.tracker;
            this.outbound
                .feed(&buf[..n.min(buf.len())], |ack, payload| {
                    if !ack {
                        tracker.sent(payload);
                    }
                });
        }
        polled
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().io).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().io).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests;
