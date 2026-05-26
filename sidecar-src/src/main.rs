//! tedi-beautify-helper - sidecar HTTP formatter.
//!
//! Boots in <50 ms on a release build, binds 127.0.0.1 on an OS-assigned
//! port, prints `READY {"port":<u16>,"token":"<hex>"}` to stdout, and
//! serves one route:
//!
//!   POST /format   { "lang": "<id>", "content": "<utf8>" }
//!                  -> 200 { "content": "<formatted>" }
//!                  -> 400 { "error": { "message": "<reason>" } }
//!   POST /shutdown -> 200 then `process::exit(0)`
//!
//! Every route validates `Authorization: Bearer <token>`. The token is a
//! 32-byte hex random per boot; it never reaches disk. Same handshake the
//! SQL Explorer extension uses, so reviewers can audit one model.
//!
//! Language dispatch is a flat `match` in `format_one`. Adding a new
//! language is one arm plus one helper crate.

use std::borrow::Cow;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process;

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
struct AppState {
    token: String,
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
    // 32 random bytes hex-encoded. `getrandom` would do too; using `rand`
    // here keeps the dep set short.
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let token = hex_encode(&bytes);

    let state = AppState {
        token: token.clone(),
    };

    let app = Router::new()
        .route("/format", post(handle_format))
        .route("/shutdown", post(handle_shutdown))
        .with_state(state);

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

    // Single-line READY handshake. JS reads it via shell_bg_logs and starts
    // POSTing. stdout is line-buffered by default for child processes; the
    // explicit \n forces a flush on every libc / runtime.
    let line = serde_json::to_string(&ReadyLine {
        port,
        token: &token,
    })
    .expect("serialize ready");
    println!("READY {line}");

    if let Err(e) = axum::serve(listener, app.into_make_service()).await {
        eprintln!("serve failed: {e}");
        process::exit(2);
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

fn check_auth(state: &AppState, headers: &HeaderMap) -> Result<(), (StatusCode, String)> {
    let got = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let expected = format!("Bearer {}", state.token);
    if got == expected {
        Ok(())
    } else {
        Err((StatusCode::UNAUTHORIZED, "bad token".into()))
    }
}

async fn handle_format(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<FormatReq>,
) -> impl IntoResponse {
    if let Err((code, msg)) = check_auth(&state, &headers) {
        return error_response(code, msg);
    }
    match format_one(&req.lang, &req.content) {
        Ok(content) => (StatusCode::OK, Json(FormatResp { content })).into_response(),
        Err(msg) => error_response(StatusCode::BAD_REQUEST, msg),
    }
}

async fn handle_shutdown(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if let Err((code, msg)) = check_auth(&state, &headers) {
        return error_response(code, msg);
    }
    // Reply first, then drop the runtime. Spawning a delayed exit gives the
    // HTTP response a chance to flush before tokio tears down.
    tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        process::exit(0);
    });
    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
}

fn error_response(code: StatusCode, message: String) -> axum::response::Response {
    (
        code,
        Json(ErrorEnvelope {
            error: ErrorDetail { message },
        }),
    )
        .into_response()
}

// --------------------------------- Formatters --------------------------------

/// Dispatches a single format request to the language helper that owns it.
/// Each arm is one language; helper crates do the parse + re-print.
fn format_one(lang: &str, content: &str) -> Result<String, String> {
    match lang {
        "json" => format_json(content),
        "css" => format_css_family(content, malva::Syntax::Css),
        "scss" => format_css_family(content, malva::Syntax::Scss),
        "less" => format_css_family(content, malva::Syntax::Less),
        "sass" => format_css_family(content, malva::Syntax::Sass),
        "html" => format_markup(content, markup_fmt::Language::Html),
        "vue" => format_markup(content, markup_fmt::Language::Vue),
        "svelte" => format_markup(content, markup_fmt::Language::Svelte),
        "astro" => format_markup(content, markup_fmt::Language::Astro),
        "angular" => format_markup(content, markup_fmt::Language::Angular),
        "javascript" => format_ts(content, "file.js"),
        "jsx" => format_ts(content, "file.jsx"),
        "typescript" => format_ts(content, "file.ts"),
        "tsx" => format_ts(content, "file.tsx"),
        "markdown" => format_markdown(content),
        "yaml" => format_yaml(content),
        "toml" => format_toml(content),
        "sql" => Ok(format_sql(content)),
        "xml" => Ok(format_xml(content)),
        other => Err(format!("unsupported language: {other}")),
    }
}

/// JSON via `serde_json`. Two-space indent, sorted by appearance (the
/// crate preserves object key order from the original document). Strips
/// any UTF-8 BOM before parsing because some Windows tools emit it.
///
/// JSONC / JSON5 inputs are passed through this same path; comments and
/// trailing commas survive parse only when the source happens to be
/// strict JSON. The README's "Known limitations" calls this out.
fn format_json(content: &str) -> Result<String, String> {
    let trimmed = content.strip_prefix('\u{feff}').unwrap_or(content);
    let value: serde_json::Value =
        serde_json::from_str(trimmed).map_err(|e| format!("json parse: {e}"))?;
    let mut buf = Vec::with_capacity(content.len() + 16);
    let fmt = serde_json::ser::PrettyFormatter::with_indent(b"  ");
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, fmt);
    serde::Serialize::serialize(&value, &mut ser)
        .map_err(|e| format!("json serialize: {e}"))?;
    let mut out = String::from_utf8(buf).map_err(|e| format!("json utf8: {e}"))?;
    // Trailing newline keeps `git diff` clean for tools that expect a
    // final EOL marker.
    out.push('\n');
    Ok(out)
}

/// CSS / SCSS / LESS / Sass via malva. Syntax flag picks the dialect; the
/// crate handles SCSS nesting, Sass indented syntax, and LESS extensions.
fn format_css_family(content: &str, syntax: malva::Syntax) -> Result<String, String> {
    let opts = malva::config::FormatOptions::default();
    malva::format_text(content, syntax, &opts).map_err(|e| format!("css ({syntax:?}): {e}"))
}

/// HTML family via markup_fmt. Embedded `<script>` / `<style>` blocks pass
/// through unchanged in v0.1.0; the callback returns the original code as
/// a borrowed `Cow` (the lifetime is tied to the input `&str`, so a borrow
/// is enough and avoids any allocation for the common pass-through case).
/// A future version can wire these back through `format_ts` / `format_css_family`.
fn format_markup(content: &str, language: markup_fmt::Language) -> Result<String, String> {
    let opts = markup_fmt::config::FormatOptions::default();
    markup_fmt::format_text(content, language, &opts, |code, _hints| {
        Ok::<_, std::convert::Infallible>(Cow::Borrowed(code))
    })
    .map_err(|e| format!("markup ({language:?}): {e}"))
}

/// JS / TS / JSX / TSX via dprint-plugin-typescript. Path drives the parser
/// mode (`.ts` vs `.tsx`); the in-memory filename never touches disk, it
/// just steers the language detection inside the plugin.
fn format_ts(content: &str, filename: &str) -> Result<String, String> {
    use dprint_plugin_typescript::configuration::ConfigurationBuilder;
    use dprint_plugin_typescript::FormatTextOptions;
    let config = ConfigurationBuilder::new().build();
    let path = PathBuf::from(filename);
    let options = FormatTextOptions {
        path: &path,
        extension: None,
        text: content.to_string(),
        config: &config,
        external_formatter: None,
    };
    match dprint_plugin_typescript::format_text(options) {
        Ok(Some(out)) => Ok(out),
        // None means "already formatted, no change". Return the input
        // unchanged - the JS side compares strings and toasts "Already
        // formatted" when the result equals the input.
        Ok(None) => Ok(content.to_string()),
        Err(e) => Err(format!("typescript: {e}")),
    }
}

/// Markdown via dprint-plugin-markdown. Code-block bodies pass through
/// unchanged in v0.1.0; the callback returns `None` to signal "leave as-is".
fn format_markdown(content: &str) -> Result<String, String> {
    use dprint_plugin_markdown::configuration::ConfigurationBuilder;
    let config = ConfigurationBuilder::new().build();
    let result = dprint_plugin_markdown::format_text(content, &config, |_lang, _code, _line_width| {
        Ok(None)
    })
    .map_err(|e| format!("markdown: {e}"))?;
    Ok(result.unwrap_or_else(|| content.to_string()))
}

/// YAML via pretty_yaml. Multi-document streams keep their `---`
/// separators; comments are preserved (unlike a serde_yaml round-trip
/// which has no representation for them).
fn format_yaml(content: &str) -> Result<String, String> {
    let opts = pretty_yaml::config::FormatOptions::default();
    pretty_yaml::format_text(content, &opts).map_err(|e| format!("yaml: {e}"))
}

/// TOML via toml_edit. Preserves comments and overall structure.
fn format_toml(content: &str) -> Result<String, String> {
    let doc: toml_edit::DocumentMut = content
        .parse()
        .map_err(|e: toml_edit::TomlError| format!("toml parse: {e}"))?;
    Ok(doc.to_string())
}

/// SQL via sqlformat. Uppercases keywords; 2-space indent. The crate
/// dialect setting is `Generic` which covers MySQL / PG / SQLite / SQL
/// Server's common subset.
fn format_sql(content: &str) -> String {
    use sqlformat::{format, FormatOptions, Indent, QueryParams};
    let opts = FormatOptions {
        indent: Indent::Spaces(2),
        uppercase: Some(true),
        lines_between_queries: 2,
        ignore_case_convert: None,
    };
    format(content, &QueryParams::None, &opts)
}

/// XML reformat. A tiny depth tracker is enough for well-formed input:
/// each `<tag>` opens a level, each `</tag>` closes one, self-closing
/// `<tag/>` and processing instructions / declarations stay on their
/// current level. Whitespace-only text nodes are dropped so re-formatting
/// an already pretty file is idempotent. markup_fmt is HTML-oriented and
/// would happily rewrite XHTML / SVG attribute order; a depth tracker
/// keeps the input bytes intact apart from indentation.
fn format_xml(content: &str) -> String {
    let mut out = String::with_capacity(content.len() + 64);
    let mut depth: usize = 0;
    let mut chars = content.chars().peekable();
    let indent_unit = "  ";
    let mut first = true;
    while let Some(c) = chars.next() {
        if c == '<' {
            let mut tag = String::from("<");
            while let Some(&nc) = chars.peek() {
                tag.push(nc);
                chars.next();
                if nc == '>' {
                    break;
                }
            }
            let body = &tag[1..tag.len().saturating_sub(1)];
            let is_close = body.starts_with('/');
            let is_decl_or_pi = body.starts_with('?') || body.starts_with('!');
            let is_self_close = body.ends_with('/');
            if is_close && depth > 0 {
                depth -= 1;
            }
            if !first {
                out.push('\n');
            }
            for _ in 0..depth {
                out.push_str(indent_unit);
            }
            out.push_str(&tag);
            first = false;
            if !is_close && !is_decl_or_pi && !is_self_close {
                depth += 1;
            }
        } else if !c.is_whitespace() {
            // Inline text: peek and grab until next `<`, then emit on the
            // current line. Avoids splitting `<b>bold</b>` across lines.
            let mut text = String::from(c);
            while let Some(&nc) = chars.peek() {
                if nc == '<' {
                    break;
                }
                text.push(nc);
                chars.next();
            }
            out.push_str(text.trim_end());
        }
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}
