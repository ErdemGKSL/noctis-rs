//! The HTTP/1.1 server behind [`FakeDiscordHttp`](super::FakeDiscordHttp).

use std::convert::Infallible;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full, LengthLimitError, Limited};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode, header};
use hyper_util::rt::TokioIo;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;

use super::{FakeResponse, MAX_BODY_BYTES, RecordedRequest, Shared};

/// The pause after a failed `accept` (e.g. out of file descriptors).
const ACCEPT_BACKOFF: Duration = Duration::from_millis(10);

/// Accepts connections until the task is aborted; dropping it drops the
/// `JoinSet`, which aborts every connection.
pub(super) async fn accept_loop(listener: TcpListener, shared: Shared) {
    let mut connections = JoinSet::new();
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                connections.spawn(serve(stream, shared.clone()));
            }
            Err(_) => tokio::time::sleep(ACCEPT_BACKOFF).await,
        }
        // Reap finished connections so the set does not grow forever.
        while connections.try_join_next().is_some() {}
    }
}

/// Serves one connection.
async fn serve(stream: TcpStream, shared: Shared) {
    let service = service_fn(move |request| {
        let shared = shared.clone();
        async move { Ok::<_, Infallible>(handle(&shared, request).await) }
    });
    // A connection error (client gone, malformed request) only ends this
    // connection.
    let _ = http1::Builder::new()
        .serve_connection(TokioIo::new(stream), service)
        .await;
}

/// Why a request body was not read.
enum BodyRejection {
    TooLarge,
    Broken,
}

/// Reads the body, logs the request, then answers it from the routes.
async fn handle(shared: &Shared, request: Request<Incoming>) -> Response<Full<Bytes>> {
    let (parts, body) = request.into_parts();
    let declared_too_large = parts
        .headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .is_some_and(|len| len > MAX_BODY_BYTES as u64);
    let body = if declared_too_large {
        // Never polled, so a client sending `expect: 100-continue` never
        // uploads it.
        Err(BodyRejection::TooLarge)
    } else {
        match Limited::new(body, MAX_BODY_BYTES).collect().await {
            Ok(collected) => Ok(collected.to_bytes()),
            Err(err) if err.downcast_ref::<LengthLimitError>().is_some() => {
                Err(BodyRejection::TooLarge)
            }
            Err(_) => Err(BodyRejection::Broken),
        }
    };
    let recorded = RecordedRequest {
        method: parts.method,
        path: parts.uri.path().to_owned(),
        query: parts.uri.query().map(str::to_owned),
        headers: parts.headers,
        body: body.as_ref().map_or_else(|_| Bytes::new(), Bytes::clone),
    };
    log(shared, recorded.clone());
    let response = match body {
        Err(BodyRejection::TooLarge) => FakeResponse::discord_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "Request entity too large",
            40005,
        ),
        Err(BodyRejection::Broken) => {
            FakeResponse::discord_error(StatusCode::BAD_REQUEST, "400: Bad Request", 0)
        }
        Ok(_) => match lookup(shared, &recorded) {
            Lookup::Route(route) => route.respond(&recorded),
            Lookup::Response(response) => response,
        },
    };
    if let Some(delay) = response.delay {
        tokio::time::sleep(delay).await;
    }
    to_hyper(response)
}

/// What a lookup found; a route is called after the lock is released.
enum Lookup {
    Route(super::Route),
    Response(FakeResponse),
}

/// Finds the answer for `request` without holding the lock afterwards.
fn lookup(shared: &Shared, request: &RecordedRequest) -> Lookup {
    let state = shared.lock();
    let Some(route_path) = request.route_path() else {
        return Lookup::Response(not_found());
    };
    if let Some(route) = state
        .routes
        .get(&(request.method.clone(), route_path.to_owned()))
    {
        return Lookup::Route(route.clone());
    }
    if request.method == hyper::Method::GET && route_path == "/gateway/bot" {
        return Lookup::Response(FakeResponse::json(
            StatusCode::OK,
            &state.gateway_bot.to_json(),
        ));
    }
    Lookup::Response(not_found())
}

/// Discord's `404` body.
fn not_found() -> FakeResponse {
    FakeResponse::discord_error(StatusCode::NOT_FOUND, "404: Not Found", 0)
}

/// Appends `request` to the log.
fn log(shared: &Shared, request: RecordedRequest) {
    shared.lock().requests.push(request);
}

/// Turns a [`FakeResponse`] into a hyper response.
fn to_hyper(response: FakeResponse) -> Response<Full<Bytes>> {
    let mut out = Response::new(Full::new(response.body));
    *out.status_mut() = response.status;
    *out.headers_mut() = response.headers;
    out
}
