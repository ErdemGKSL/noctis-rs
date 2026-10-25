//! Test helpers: a tiny WAV and a local HTTP/1.1 media server that records
//! request heads.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};

use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpListener;

/// 0.1 s of 16-bit mono 48 kHz silence, as a WAV file.
pub(crate) fn wav() -> Vec<u8> {
    const RATE: u32 = 48_000;
    let samples = RATE / 10;
    let data_len = samples * 2;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16_u32.to_le_bytes());
    out.extend_from_slice(&1_u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1_u16.to_le_bytes()); // mono
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes());
    out.extend_from_slice(&2_u16.to_le_bytes());
    out.extend_from_slice(&16_u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.resize(44 + data_len as usize, 0);
    out
}

/// A local media server on `127.0.0.1:0`:
/// - `/wav`: the WAV;
/// - `/hop/<n>`: a 302 to `/hop/<n-1>`, and `/hop/0` to `/wav`;
/// - `/discord`: a 302 to `https://discord.com/api/v10/users/@me`;
/// - anything else: 404.
pub(crate) struct MediaServer {
    pub(crate) addr: SocketAddr,
    heads: Arc<Mutex<Vec<String>>>,
}

impl MediaServer {
    pub(crate) async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap_or_else(|e| panic!("bind: {e}"));
        let addr = listener
            .local_addr()
            .unwrap_or_else(|e| panic!("addr: {e}"));
        let heads = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&heads);
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let recorded = Arc::clone(&recorded);
                tokio::spawn(async move {
                    let mut head = Vec::new();
                    let mut buf = [0_u8; 1024];
                    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                        match stream.read(&mut buf).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => head.extend_from_slice(&buf[..n]),
                        }
                    }
                    let head = String::from_utf8_lossy(&head).into_owned();
                    let path = head
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or_default()
                        .to_owned();
                    recorded
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push(head);
                    let response = respond(&path);
                    let _ = stream.write_all(&response).await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        Self { addr, heads }
    }

    /// `http://<addr><path>`.
    pub(crate) fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    /// Every request head received so far.
    pub(crate) fn heads(&self) -> Vec<String> {
        self.heads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

fn redirect(location: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 302 Found\r\nlocation: {location}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
    )
    .into_bytes()
}

fn respond(path: &str) -> Vec<u8> {
    if path == "/wav" {
        let body = wav();
        let mut out = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: audio/wav\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        out.extend_from_slice(&body);
        return out;
    }
    if path == "/discord" {
        return redirect("https://discord.com/api/v10/users/@me");
    }
    if let Some(n) = path
        .strip_prefix("/hop/")
        .and_then(|n| n.parse::<u32>().ok())
    {
        return match n {
            0 => redirect("/wav"),
            n => redirect(&format!("/hop/{}", n - 1)),
        };
    }
    b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_vec()
}
