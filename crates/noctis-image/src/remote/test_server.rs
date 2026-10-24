//! A raw HTTP/1.1 test server on a loopback tokio `TcpListener`: it answers
//! every request with the [`Reply`] its handler picks for the request
//! target, and records the requests, the most requests in flight at once
//! and the requests whose client went away before the reply.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// What the server answers.
#[derive(Debug, Clone)]
pub(crate) enum Reply {
    /// `200` with `body` and its `content-length`.
    Ok(Vec<u8>),
    /// `200` with `body`, no `content-length`; the connection closes after.
    Unsized(Vec<u8>),
    /// `200` claiming `content-length: <n>`, then nothing until the client
    /// goes away.
    Claim(u64),
    /// This status with an empty body.
    Status(u16),
    /// `301` to `location`.
    Redirect(String),
    /// The reply, after a delay (aborted when the client goes away first).
    After(Duration, Box<Reply>),
}

/// One recorded request.
#[derive(Debug, Clone)]
pub(crate) struct Request {
    /// The request target (`/avatars/1/x.webp?size=256`).
    pub(crate) target: String,
    /// The request head, lowercased.
    pub(crate) head: String,
}

type Handler = Box<dyn Fn(&str) -> Reply + Send + Sync>;

struct State {
    handler: Handler,
    requests: Mutex<Vec<Request>>,
    in_flight: AtomicUsize,
    max_in_flight: AtomicUsize,
    aborted: AtomicUsize,
}

/// A running test server; it stops with the test's runtime.
#[derive(Clone)]
pub(crate) struct Server {
    addr: SocketAddr,
    state: Arc<State>,
}

impl Server {
    /// Starts a server answering with `handler(target)`.
    pub(crate) async fn start(handler: impl Fn(&str) -> Reply + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let state = Arc::new(State {
            handler: Box::new(handler),
            requests: Mutex::new(Vec::new()),
            in_flight: AtomicUsize::new(0),
            max_in_flight: AtomicUsize::new(0),
            aborted: AtomicUsize::new(0),
        });
        let accept = Arc::clone(&state);
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(serve(stream, Arc::clone(&accept)));
            }
        });
        Self { addr, state }
    }

    /// `http://127.0.0.1:<port>`.
    pub(crate) fn origin(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// `<origin><path>`.
    pub(crate) fn url(&self, path: &str) -> String {
        format!("{}{path}", self.origin())
    }

    /// Every request so far.
    pub(crate) fn requests(&self) -> Vec<Request> {
        self.state.requests.lock().unwrap().clone()
    }

    /// The most requests that were in flight at once.
    pub(crate) fn max_in_flight(&self) -> usize {
        self.state.max_in_flight.load(Ordering::SeqCst)
    }

    /// The requests whose client went away before the reply.
    pub(crate) fn aborted(&self) -> usize {
        self.state.aborted.load(Ordering::SeqCst)
    }
}

/// Reads one request head, answers it and closes the connection.
async fn serve(mut stream: TcpStream, state: Arc<State>) {
    let mut head = Vec::new();
    let mut buf = [0; 1024];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(n) => head.extend_from_slice(&buf[..n]),
        }
    }
    let head = String::from_utf8_lossy(&head).to_ascii_lowercase();
    let target = head.split(' ').nth(1).unwrap_or_default().to_owned();
    let mut reply = (state.handler)(&target);
    state.requests.lock().unwrap().push(Request {
        target,
        head: head.clone(),
    });
    let now = state.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
    state.max_in_flight.fetch_max(now, Ordering::SeqCst);
    loop {
        match reply {
            Reply::After(delay, next) => {
                tokio::select! {
                    () = tokio::time::sleep(delay) => reply = *next,
                    _ = stream.read(&mut buf) => {
                        state.aborted.fetch_add(1, Ordering::SeqCst);
                        state.in_flight.fetch_sub(1, Ordering::SeqCst);
                        return;
                    }
                }
            }
            Reply::Claim(len) => {
                let head = format!("HTTP/1.1 200 OK\r\ncontent-length: {len}\r\n\r\n");
                let _ = stream.write_all(head.as_bytes()).await;
                let _ = stream.read(&mut buf).await;
                break;
            }
            Reply::Ok(body) => {
                let head = format!(
                    "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes()).await;
                let _ = stream.write_all(&body).await;
                break;
            }
            Reply::Unsized(body) => {
                let _ = stream
                    .write_all(b"HTTP/1.1 200 OK\r\nconnection: close\r\n\r\n")
                    .await;
                let _ = stream.write_all(&body).await;
                break;
            }
            Reply::Status(status) => {
                let head = format!(
                    "HTTP/1.1 {status} X\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                );
                let _ = stream.write_all(head.as_bytes()).await;
                break;
            }
            Reply::Redirect(location) => {
                let head = format!(
                    "HTTP/1.1 301 Moved\r\nlocation: {location}\r\ncontent-length: 0\r\n\
                     connection: close\r\n\r\n"
                );
                let _ = stream.write_all(head.as_bytes()).await;
                break;
            }
        }
    }
    let _ = stream.shutdown().await;
    state.in_flight.fetch_sub(1, Ordering::SeqCst);
}

/// A `width`x`height` PNG of one `rgba` colour.
pub(crate) fn png(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(width, height, image::Rgba(rgba));
    let mut out = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

/// A 2x2 animated WebP: red for 0..500 ms, blue for 500..1000 ms.
pub(crate) fn animated_webp() -> Vec<u8> {
    use takumi::prelude::{AnimatedWebpOptions, AnimationFrame, Bitmap};

    let frame = |rgba: [u8; 4]| {
        let bitmap = Bitmap::from_raw(2, 2, rgba.repeat(4)).unwrap();
        AnimationFrame::new(bitmap, 500)
    };
    let frames = vec![frame([255, 0, 0, 255]), frame([0, 0, 255, 255])];
    let mut out = Vec::new();
    takumi::write_animated_webp(
        std::borrow::Cow::Owned(frames),
        &mut out,
        AnimatedWebpOptions::builder().lossless(true).build(),
    )
    .unwrap();
    out
}

/// Every log line of the current thread while the guard lives.
#[derive(Clone, Default)]
pub(crate) struct Lines(Arc<Mutex<Vec<u8>>>);

impl Lines {
    pub(crate) fn warnings(&self) -> Vec<String> {
        String::from_utf8(self.0.lock().unwrap().clone())
            .unwrap()
            .lines()
            .filter(|line| line.contains("WARN") && line.contains("noctis::image"))
            .map(str::to_owned)
            .collect()
    }
}

impl std::io::Write for Lines {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Lines {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// A thread-local capturing subscriber. A second registered dispatcher
/// keeps tracing-core from caching a callsite's interest from whichever
/// subscriber saw it first (as in noctis-testkit's `capture_lines`).
pub(crate) fn capture_lines() -> (Lines, impl Sized) {
    use tracing_subscriber::util::SubscriberInitExt;

    let lines = Lines::default();
    let second = tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
    let guard = tracing_subscriber::fmt()
        .with_max_level(tracing_subscriber::filter::LevelFilter::DEBUG)
        .with_ansi(false)
        .with_writer(lines.clone())
        .finish()
        .set_default();
    tracing::callsite::rebuild_interest_cache();
    (lines, (guard, second))
}
