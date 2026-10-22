//! Raw REST requests (FR-31, FR-52): [`RawHttp::raw`] sends a request to an
//! endpoint twilight does not model through the App's one twilight client,
//! so it passes the Discord edge, the client's rate limiter (off when a
//! Cluster HTTP proxy is configured), its timeout and its token. There is no
//! second client (AD-24).

use core::fmt;
use std::future::{Future, IntoFuture};
use std::pin::Pin;

use bytes::Bytes;
use serde::Serialize;
use serde::de::DeserializeOwned;
use twilight_http::Client;
use twilight_http::error::ErrorType;
use twilight_http::request::{Form, RequestBuilder};
use twilight_http::response::DeserializeBodyError;

pub use twilight_http::request::Method;

mod sealed {
    /// Only `twilight_http::Client` implements [`RawHttp`](super::RawHttp).
    pub trait Sealed {}

    impl Sealed for twilight_http::Client {}
}

/// Adds [`raw`](RawHttp::raw) to the App's twilight client: a request to any
/// path under `/api/v10/`, for endpoints twilight does not model.
///
/// It is implemented only for `twilight_http::Client`, so through `Deref`
/// it works on `ctx.http()` and `app.http()` (an `&Arc<Client>`). The
/// request goes through `Client::request`: the configured edge or
/// `http_proxy`, the client's rate limiter, its timeout and (unless
/// [`RawRequest::without_token`]) its bot token. Only 2xx responses are a
/// [`RawResponse`]; the Responder never sees these requests, so prefer
/// `ctx.respond`/`ctx.update` for interaction replies.
///
/// ```no_run
/// use noctis_core::{Method, RawHttp, RawHttpError};
///
/// # async fn pins(http: &std::sync::Arc<twilight_http::Client>) -> Result<(), RawHttpError> {
/// let response = http.raw(Method::Get, "channels/1/pins").await?;
/// let pins: serde_json::Value = response.json()?;
/// # let _ = pins;
/// # Ok(())
/// # }
/// ```
pub trait RawHttp: sealed::Sealed {
    /// A `method` request to `path`, the part of the URL after `/api/v10/`
    /// with its query (`channels/1/messages?limit=2`). Nothing is sent
    /// until it is awaited.
    ///
    /// The path rule (checked on await, in this order; Noctis's own rule):
    /// not empty; no leading `/`; only bytes `0x21..=0x7E` and neither `#`
    /// nor `\` (percent-encode anything else); no empty segment before the
    /// first `?` (`a//b`, a trailing `/`, a leading `?`); no `.` or `..`
    /// segment; a first segment other than `api` (any case). Percent escapes
    /// are passed on unchanged.
    fn raw(&self, method: Method, path: impl Into<String>) -> RawRequest<'_>;
}

impl RawHttp for Client {
    fn raw(&self, method: Method, path: impl Into<String>) -> RawRequest<'_> {
        RawRequest {
            client: self,
            method,
            path: path.into(),
            body: Body::None,
            token: true,
        }
    }
}

/// The body of a [`RawRequest`].
enum Body {
    None,
    Bytes(Vec<u8>),
    Form(Form),
    /// A [`RawRequest::json`] value that did not serialize.
    Encode(serde_json::Error),
}

/// A raw request, sent when awaited ([`RawHttp::raw`]).
///
/// [`json`](Self::json), [`body`](Self::body) and [`form`](Self::form) each
/// replace any body set before.
#[must_use = "a raw request is sent only when awaited"]
pub struct RawRequest<'a> {
    client: &'a Client,
    method: Method,
    path: String,
    body: Body,
    token: bool,
}

impl RawRequest<'_> {
    /// Sends `value` serialized as JSON (`content-type: application/json`).
    /// A serialization error is returned on await, as
    /// [`RawHttpError::Encode`], and nothing is sent.
    pub fn json<T: Serialize + ?Sized>(mut self, value: &T) -> Self {
        self.body = match serde_json::to_vec(value) {
            Ok(bytes) => Body::Bytes(bytes),
            Err(error) => Body::Encode(error),
        };
        self
    }

    /// Sends `bytes`, JSON, verbatim (`content-type: application/json`).
    pub fn body(mut self, bytes: Vec<u8>) -> Self {
        self.body = Body::Bytes(bytes);
        self
    }

    /// Sends the multipart `form`.
    pub fn form(mut self, form: Form) -> Self {
        self.body = Body::Form(form);
        self
    }

    /// Sends no `Authorization` header, for webhook and interaction-token
    /// routes.
    pub fn without_token(mut self) -> Self {
        self.token = false;
        self
    }
}

impl fmt::Debug for RawRequest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The path may hold a webhook or interaction token.
        let body = match &self.body {
            Body::None => "none",
            Body::Bytes(_) => "bytes",
            Body::Form(_) => "form",
            Body::Encode(_) => "encode error",
        };
        f.debug_struct("RawRequest")
            .field("method", &self.method)
            .field("body", &body)
            .field("token", &self.token)
            .finish_non_exhaustive()
    }
}

impl<'a> IntoFuture for RawRequest<'a> {
    type Output = Result<RawResponse, RawHttpError>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send + 'a>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(send(self))
    }
}

async fn send(request: RawRequest<'_>) -> Result<RawResponse, RawHttpError> {
    let RawRequest {
        client,
        method,
        path,
        body,
        token,
    } = request;
    if let Err(reason) = check_path(&path) {
        return Err(RawHttpError::InvalidPath { path, reason });
    }
    let mut builder = RequestBuilder::raw(method, path);
    match body {
        Body::None => {}
        Body::Bytes(bytes) => builder = builder.body(bytes),
        Body::Form(form) => builder = builder.form(form),
        Body::Encode(error) => return Err(RawHttpError::Encode(error)),
    }
    let built = builder
        .use_authorization_token(token)
        .build()
        .map_err(RawHttpError::Http)?;
    let response = client
        .request::<serde_json::Value>(built)
        .await
        .map_err(from_twilight)?;
    let status = response.status().get();
    let headers = response
        .headers()
        .map(|(name, value)| (name.to_owned(), value.to_vec()))
        .collect();
    let body = Bytes::from(response.bytes().await.map_err(RawHttpError::Body)?);
    Ok(RawResponse {
        status,
        headers,
        body,
    })
}

/// A non-2xx status becomes [`RawHttpError::Status`].
fn from_twilight(error: twilight_http::Error) -> RawHttpError {
    if !matches!(error.kind(), ErrorType::Response { .. }) {
        return RawHttpError::Http(error);
    }
    match error.into_parts() {
        (ErrorType::Response { body, status, .. }, _) => RawHttpError::Status {
            status: status.get(),
            body: Bytes::from(body),
        },
        // Checked just above.
        (_, _) => unreachable!("the error kind is `Response`"),
    }
}

/// The path rule of [`RawHttp::raw`]: `Err` names the first broken part.
fn check_path(path: &str) -> Result<(), &'static str> {
    if path.is_empty() {
        return Err("the path is empty");
    }
    if path.starts_with('/') {
        return Err("the path starts with `/`; give the part after `/api/v10/`");
    }
    if path
        .bytes()
        .any(|b| !(0x21..=0x7E).contains(&b) || b == b'#' || b == b'\\')
    {
        return Err(
            "the path holds a space, a control or non-ASCII character, `#` or `\\`; percent-encode it",
        );
    }
    let route = path.split_once('?').map_or(path, |(route, _)| route);
    let mut segments = route.split('/');
    if segments.clone().any(str::is_empty) {
        return Err("the path has an empty segment (`//`, a trailing `/` or a leading `?`)");
    }
    if segments.clone().any(|s| s == "." || s == "..") {
        return Err("the path has a `.` or `..` segment");
    }
    if segments
        .next()
        .is_some_and(|s| s.eq_ignore_ascii_case("api"))
    {
        return Err("the path starts with `api`; give the part after `/api/v10/`");
    }
    Ok(())
}

/// A 2xx response to a [`RawRequest`].
#[derive(Clone, PartialEq, Eq)]
pub struct RawResponse {
    status: u16,
    headers: Vec<(String, Vec<u8>)>,
    body: Bytes,
}

impl RawResponse {
    /// The HTTP status, 200 to 299.
    pub fn status(&self) -> u16 {
        self.status
    }

    /// The first value of header `name` (ASCII case-insensitive); `None`
    /// when it is missing or not UTF-8.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .and_then(|(_, value)| core::str::from_utf8(value).ok())
    }

    /// The body, as sent (empty for a 204).
    pub fn bytes(&self) -> &[u8] {
        &self.body
    }

    /// The body, as sent.
    pub fn into_bytes(self) -> Bytes {
        self.body
    }

    /// The body decoded as JSON.
    ///
    /// # Errors
    ///
    /// [`RawHttpError::Decode`] when the body is empty or is not a `T`.
    pub fn json<T: DeserializeOwned>(&self) -> Result<T, RawHttpError> {
        serde_json::from_slice(&self.body).map_err(RawHttpError::Decode)
    }
}

impl fmt::Debug for RawResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RawResponse")
            .field("status", &self.status)
            .field("body_len", &self.body.len())
            .finish_non_exhaustive()
    }
}

/// Why a [`RawRequest`] failed. It converts into a `HandlerError` with `?`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RawHttpError {
    /// The path breaks the rule of [`RawHttp::raw`]; nothing was sent.
    #[error("raw request: invalid path `{path}`: {reason}")]
    InvalidPath {
        /// The rejected path.
        path: String,
        /// The broken part of the rule.
        reason: &'static str,
    },
    /// The [`RawRequest::json`] value did not serialize; nothing was sent.
    #[error("raw request: the JSON body could not be serialized")]
    Encode(#[source] serde_json::Error),
    /// Discord (or the proxy) answered with a status outside 2xx.
    #[error("raw request: status {status}")]
    Status {
        /// The HTTP status.
        status: u16,
        /// The response body, such as Discord's `{"code": …, "message": …}`.
        body: Bytes,
    },
    /// The response body could not be read.
    #[error("raw request: the response body could not be read")]
    Body(#[source] DeserializeBodyError),
    /// [`RawResponse::json`] could not decode the body.
    #[error("raw request: the response body is not the expected JSON")]
    Decode(#[source] serde_json::Error),
    /// Any other failure of the client: a timeout, the transport, an
    /// invalidated token.
    #[error("raw request: the request failed")]
    Http(#[source] twilight_http::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_path_rejects_each_rule() {
        let cases = [
            ("", "empty"),
            ("/channels/1", "starts with `/`"),
            ("a b", "percent-encode"),
            ("a/ü", "percent-encode"),
            ("a#f", "percent-encode"),
            ("a\\b", "percent-encode"),
            ("a\n", "percent-encode"),
            ("a//b", "empty segment"),
            ("a/", "empty segment"),
            ("?x=1", "empty segment"),
            ("a/./b", "`.` or `..`"),
            ("a/../b", "`.` or `..`"),
            ("api/v10/x", "starts with `api`"),
            ("API/x", "starts with `api`"),
        ];
        for (path, reason) in cases {
            let got = check_path(path).expect_err(path);
            assert!(got.contains(reason), "{path:?}: {got}");
        }
    }

    #[test]
    fn raw_path_accepts_edges() {
        for path in [
            "@me",
            "a",
            "a?",
            "a?b/c//d",
            "applications/@me",
            "channels/1/messages?limit=2",
            "channels/1/messages/2/reactions/%F0%9F%91%8D/@me",
            "apis/x",
        ] {
            assert_eq!(check_path(path), Ok(()), "{path:?}");
        }
    }

    #[test]
    fn raw_response_header_lookup() {
        let response = RawResponse {
            status: 200,
            headers: vec![
                ("x-ratelimit-bucket".to_owned(), b"b".to_vec()),
                ("x-bin".to_owned(), vec![0xFF]),
                ("x-ratelimit-bucket".to_owned(), b"second".to_vec()),
            ],
            body: Bytes::new(),
        };
        assert_eq!(response.header("X-RateLimit-Bucket"), Some("b"));
        assert_eq!(response.header("x-bin"), None);
        assert_eq!(response.header("missing"), None);
        assert!(matches!(
            response.json::<serde_json::Value>(),
            Err(RawHttpError::Decode(_))
        ));
    }

    #[test]
    fn raw_errors_start_with_raw_request() {
        let invalid = RawHttpError::InvalidPath {
            path: String::new(),
            reason: "the path is empty",
        };
        let status = RawHttpError::Status {
            status: 404,
            body: Bytes::new(),
        };
        for error in [invalid, status] {
            assert!(error.to_string().starts_with("raw request:"), "{error}");
        }
    }

    const _: () = {
        const fn send<T: Send>() {}
        send::<RawRequest<'_>>();
        send::<RawResponse>();
        send::<RawHttpError>();
    };
}
