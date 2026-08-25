//! tedi-beautify-helper - sidecar HTTP formatter.
//!
//! Boots in <50 ms on a release build, binds 127.0.0.1 on an OS-assigned
//! port, prints `READY {"port":<u16>,"token":"<hex>"}` to stdout, and
//! serves two routes:
//!
//!   POST /format   { "lang": "<id>", "content": "<utf8>" }
//!                  -> 200 { "content": "<formatted>" }
//!                  -> 400 { "error": { "message": "<reason>" } }
//!   POST /shutdown -> 200 then `process::exit(0)`
//!
//! Every route validates `Authorization: Bearer <token>` in constant time.
//! The token is 32 random bytes hex-encoded per boot; it never reaches disk.
//! Same handshake the SQL Explorer extension uses, so reviewers audit one
//! model.
//!
//! Three things keep one bad buffer from taking the process down with it:
//! the body is read as bytes and parsed only after auth, formatting runs on
//! a blocking thread inside `catch_unwind` (hence no `panic = "abort"`), and
//! an idle watchdog exits the process if the host that spawned it dies
//! without ever calling `/shutdown`.

mod format;

use std::net::SocketAddr;
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::{
        header::{AUTHORIZATION, CONTENT_TYPE},
        HeaderMap, Method, StatusCode,
    },
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tower_http::cors::{Any, CorsLayer};

/// axum's default is 2 MB, which is smaller than the minified bundles people
/// reach for a beautifier to read in the first place. The buffer is already
/// resident in the editor, so the ceiling is only here to bound what an
/// unauthenticated caller can make us allocate.
const MAX_BODY_BYTES: usize = 64 * 1024 * 1024;

/// If the host is killed rather than closed, `deactivate` never runs and
/// nothing kills us: on Windows the job object catches it, on macOS / Linux
/// the process would sit on a loopback port forever. Exit on our own once
/// nobody has asked for anything in a long while.
const IDLE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const IDLE_TICK: Duration = Duration::from_secs(60);

#[derive(Clone)]
struct AppState {
    token: Arc<String>,
    boot: Instant,
    /// Seconds since `boot` at the last authenticated request.
    last_hit: Arc<AtomicU64>,
}

impl AppState {
    fn touch(&self) {
        self.last_hit
            .store(self.boot.elapsed().as_secs(), Ordering::Relaxed);
    }
}

#[derive(Deserialize)]
struct FormatReq {
    lang: String,
    content: String,
}

#[derive(Serialize)]
struct FormatResp {
    content: String,
}

#[derive(Serialize)]
struct ErrorEnvelope {
    error: ErrorDetail,
}

#[derive(Serialize)]
struct ErrorDetail {
    message: String,
}

#[derive(Serialize)]
struct ReadyLine<'a> {
    port: u16,
    token: &'a str,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    let mut bytes = [0u8; 32];
    if let Err(e) = getrandom::fill(&mut bytes) {
        eprintln!("entropy unavailable: {e}");
        process::exit(2);
    }
    let token = hex_encode(&bytes);

    let state = AppState {
        token: Arc::new(token.clone()),
        boot: Instant::now(),
        last_hit: Arc::new(AtomicU64::new(0)),
    };

    // CORS: WebView2 sends a preflight OPTIONS for our POST /format
    // because Content-Type: application/json + Authorization are both
    // non-safelisted. `Authorization` MUST be listed explicitly: per the
    // Fetch spec the `*` wildcard in Access-Control-Allow-Headers does NOT
    // cover Authorization, so `.allow_headers(Any)` (which emits `*`) makes
    // WebView2 reject the preflight and the extension sees
    // `TypeError: Failed to fetch` on every /format call. Listing the two
    // headers we actually send fixes it. Bearer token is the real auth;
    // CORS just controls what the browser will read back.
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([AUTHORIZATION, CONTENT_TYPE]);

    let app = Router::new()
        .route("/format", post(handle_format))
        .route("/shutdown", post(handle_shutdown))
        .with_state(state.clone())
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(cors);

    // Port 0 -> OS picks. Read the bound port back so the JS side knows
    // where to connect.
    let addr: SocketAddr = "127.0.0.1:0".parse().expect("loopback");
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("bind failed: {e}");
            process::exit(2);
        }
    };
    let port = listener.local_addr().expect("local_addr").port();

    tokio::spawn(idle_watchdog(state));

    // Single-line READY handshake. JS reads it via shell_bg_logs and starts
    // POSTing. stdout is line-buffered by default for child processes; the
    // explicit \n forces a flush on every libc / runtime.
    let line = serde_json::to_string(&ReadyLine {
        port,
        token: &token,
    })
    .expect("serialize ready");
    println!("READY {line}");

    if let Err(e) = axum::serve(listener, app).await {
        eprintln!("serve failed: {e}");
        process::exit(2);
    }
}

/// Split out from the loop so the arithmetic - the part that can be wrong -
/// is testable without waiting half an hour for a tick.
fn is_idle(uptime_secs: u64, last_hit_secs: u64) -> bool {
    uptime_secs.saturating_sub(last_hit_secs) >= IDLE_TIMEOUT.as_secs()
}

async fn idle_watchdog(state: AppState) {
    loop {
        tokio::time::sleep(IDLE_TICK).await;
        let last_hit = state.last_hit.load(Ordering::Relaxed);
        if is_idle(state.boot.elapsed().as_secs(), last_hit) {
            process::exit(0);
        }
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// Length-independent, content-independent byte compare. Loopback makes a
/// remote timing attack far-fetched, but this is the whole authentication
/// check - it costs four lines to not be the interesting part of the file.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn check_auth(state: &AppState, headers: &HeaderMap) -> bool {
    let got = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let expected = format!("Bearer {}", state.token);
    ct_eq(got.as_bytes(), expected.as_bytes())
}

/// The body arrives as `Bytes`, not `Json<FormatReq>`, so auth is checked
/// before anything parses a 64 MB payload.
async fn handle_format(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    if !check_auth(&state, &headers) {
        return error_response(StatusCode::UNAUTHORIZED, "bad token".into());
    }
    state.touch();

    let req: FormatReq = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, format!("bad request: {e}")),
    };

    // Formatting is CPU-bound and can run for a second on a large file, so it
    // does not belong on a runtime worker. `catch_unwind` turns a parser panic
    // into a 400 for that one file instead of a dead sidecar and a broken
    // extension until the user toggles it off and on.
    let joined = tokio::task::spawn_blocking(move || {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            format::format_document(&req.lang, &req.content)
        }))
    })
    .await;

    match joined {
        Ok(Ok(Ok(content))) => (StatusCode::OK, Json(FormatResp { content })).into_response(),
        Ok(Ok(Err(msg))) => error_response(StatusCode::BAD_REQUEST, msg),
        Ok(Err(_)) => error_response(
            StatusCode::BAD_REQUEST,
            "the formatter crashed on this file; it was left unchanged".into(),
        ),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, format!("join: {e}")),
    }
}

async fn handle_shutdown(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !check_auth(&state, &headers) {
        return error_response(StatusCode::UNAUTHORIZED, "bad token".into());
    }
    // Reply first, then exit. The delay gives the HTTP response a chance to
    // flush before tokio tears the runtime down under it.
    tokio::spawn(async {
        tokio::time::sleep(Duration::from_millis(50)).await;
        process::exit(0);
    });
    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
}

fn error_response(code: StatusCode, message: String) -> Response {
    (
        code,
        Json(ErrorEnvelope {
            error: ErrorDetail { message },
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ct_eq_matches_only_identical_bytes() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"ab"));
        assert!(ct_eq(b"", b""));
    }

    #[test]
    fn hex_encode_is_lowercase_and_double_width() {
        assert_eq!(hex_encode(&[0x00, 0x0f, 0xff]), "000fff");
    }

    #[test]
    fn idle_is_measured_from_the_last_request_not_from_boot() {
        let timeout = IDLE_TIMEOUT.as_secs();
        // Never used, up for longer than the timeout -> go away.
        assert!(is_idle(timeout, 0));
        assert!(!is_idle(timeout - 1, 0));
        // Busy the whole time: uptime is way past the timeout but the last
        // request is recent, so a formatter someone is actually using never
        // exits from under them.
        assert!(!is_idle(timeout * 10, timeout * 10));
        assert!(is_idle(timeout * 10, timeout * 9));
        // A clock that reads backwards must not be read as "idle forever".
        assert!(!is_idle(0, 99_999));
    }
}
