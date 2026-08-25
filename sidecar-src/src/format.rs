//! Language dispatch + the embedded-code plumbing that makes the HTML / Vue /
//! Markdown / tagged-template paths format their inner blocks instead of
//! passing them through untouched.
//!
//! Everything here is pure: `&str` in, `Result<String, String>` out, no IO and
//! no host calls, so `main.rs` can run it on a blocking thread inside
//! `catch_unwind` without any further ceremony.
//!
//! Adding a language is one arm in [`format_one`] plus - if it can also appear
//! inside another document - one arm in [`lang_for_ext`].

use std::borrow::Cow;
use std::cell::Cell;
use std::path::{Path, PathBuf};

/// Prettier's default, and every one of these crates' default too. Embedded
/// blocks get a narrower width handed to them by their parent formatter.
pub const PRINT_WIDTH: usize = 80;

/// A block narrower than this prints one token per line, which is worse than
/// leaving it alone. `markup_fmt` can hand us a saturated-to-zero width for a
/// deeply indented `<style>`, so clamp rather than trust it.
const MIN_EMBED_WIDTH: usize = 20;

/// How deep an embedded chain may go: markdown -> html -> script -> css`...`
/// is 4. Deeper than that is a pathological document, and each level is a real
/// recursive parse, so the guard is what stops a hand-crafted file from
/// overflowing the stack (which no `catch_unwind` can recover from).
const MAX_EMBED_DEPTH: u8 = 4;

thread_local! {
    static EMBED_DEPTH: Cell<u8> = const { Cell::new(0) };
}

/// RAII depth counter. `Drop` rather than a manual decrement so an unwinding
/// panic inside a nested formatter can't leave the counter pinned high and
/// silently disable embedded formatting for the rest of the process.
struct DepthGuard;

impl DepthGuard {
    fn enter() -> Option<Self> {
        EMBED_DEPTH.with(|d| {
            let n = d.get();
            if n >= MAX_EMBED_DEPTH {
                return None;
            }
            d.set(n + 1);
            Some(DepthGuard)
        })
    }
}

impl Drop for DepthGuard {
    fn drop(&mut self) {
        EMBED_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

// ------------------------------- Byte shape ----------------------------------

/// The two things every formatter in this file throws away and a Windows user
/// notices immediately: the UTF-8 BOM and CRLF line endings. Reformatting a
/// CRLF file into LF turns a two-line edit into a whole-file diff, so the
/// shape is detached before parsing and re-applied after printing.
struct Shape {
    bom: bool,
    crlf: bool,
}

impl Shape {
    fn detach(src: &str) -> (Cow<'_, str>, Self) {
        let (bom, body) = match src.strip_prefix('\u{feff}') {
            Some(rest) => (true, rest),
            None => (false, src),
        };
        let crlf = body.contains("\r\n");
        let body = if crlf {
            Cow::Owned(body.replace("\r\n", "\n"))
        } else {
            Cow::Borrowed(body)
        };
        (body, Shape { bom, crlf })
    }

    fn reattach(&self, mut out: String) -> String {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        if self.crlf {
            // Normalise first: a formatter that already emitted CRLF (none do
            // today, but the configs are theirs to change) would otherwise
            // come back as CR CR LF.
            out = out.replace("\r\n", "\n").replace('\n', "\r\n");
        }
        if self.bom {
            out.insert(0, '\u{feff}');
        }
        out
    }
}

// -------------------------------- Entry point --------------------------------

/// Format a whole document. This is the only function `main.rs` calls.
pub fn format_document(lang: &str, content: &str) -> Result<String, String> {
    let (body, shape) = Shape::detach(content);
    let out = format_one(lang, &body, PRINT_WIDTH)?;
    Ok(shape.reattach(out))
}

/// Dispatches one chunk of source to the formatter that owns it. Called for
/// whole documents and, through [`format_embedded`], for every block found
/// inside one.
fn format_one(lang: &str, content: &str, width: usize) -> Result<String, String> {
    use markup_fmt::Language as M;
    match lang {
        "json" => format_json(content, "file.json", width),
        // Comments and trailing commas parse in this mode and survive the
        // re-print, which is the whole reason .jsonc is a separate id.
        "jsonc" => format_json(content, "file.jsonc", width),
        "css" => format_css(content, malva::Syntax::Css, width),
        "scss" => format_css(content, malva::Syntax::Scss, width),
        "less" => format_css(content, malva::Syntax::Less, width),
        "sass" => format_css(content, malva::Syntax::Sass, width),
        "html" => format_markup(content, M::Html, width),
        "vue" => format_markup(content, M::Vue, width),
        "svelte" => format_markup(content, M::Svelte, width),
        "astro" => format_markup(content, M::Astro, width),
        "angular" => format_markup(content, M::Angular, width),
        "jinja" => format_markup(content, M::Jinja, width),
        "vento" => format_markup(content, M::Vento, width),
        "mustache" => format_markup(content, M::Mustache, width),
        // markup_fmt gained a real XML mode, so the hand-rolled depth tracker
        // this used to use is gone. That one split a tag at the first `>`,
        // which lands inside `<a title="x > y">` and inside `<!-- a > b -->`,
        // and it injected a newline there - editing the document, not just its
        // whitespace. A real parser cannot do that.
        "xml" => format_markup(content, M::Xml, width),
        "javascript" => format_ts(content, "file.js", width),
        "jsx" => format_ts(content, "file.jsx", width),
        "typescript" => format_ts(content, "file.ts", width),
        "tsx" => format_ts(content, "file.tsx", width),
        "markdown" => format_markdown(content, width),
        "yaml" => format_yaml(content, width),
        "toml" => format_toml(content),
        "sql" => Ok(format_sql(content)),
        other => Err(format!("unsupported language: {other}")),
    }
}

// ------------------------------ Embedded blocks ------------------------------

/// Maps the file-extension hints the parent formatters hand us onto our own
/// language ids. `markup_fmt` passes `<script lang>` / `<style lang>` values,
/// `dprint-plugin-markdown` passes a fence's info string, and
/// `dprint-plugin-typescript` passes a tagged template's tag name.
///
/// `None` means "no formatter for this", which every caller reads as "leave
/// the block exactly as the author wrote it".
fn lang_for_ext(ext: &str) -> Option<&'static str> {
    Some(match ext.trim().to_ascii_lowercase().as_str() {
        "js" | "mjs" | "cjs" | "javascript" | "babel" => "javascript",
        "jsx" => "jsx",
        "ts" | "mts" | "cts" | "typescript" => "typescript",
        "tsx" => "tsx",
        "css" | "postcss" | "pcss" | "styled" => "css",
        "scss" => "scss",
        "less" => "less",
        "sass" => "sass",
        "json" => "json",
        "jsonc" | "json5" => "jsonc",
        "html" | "htm" => "html",
        "vue" => "vue",
        "svelte" => "svelte",
        "astro" => "astro",
        "xml" | "svg" => "xml",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "md" | "markdown" => "markdown",
        "sql" => "sql",
        _ => return None,
    })
}

/// Format one embedded block. `None` - unknown language, too deep, or the
/// block does not parse - always means "leave it alone": a broken `<script>`
/// must not fail the whole HTML document.
fn format_embedded(ext: &str, code: &str, width: usize) -> Option<String> {
    let _depth = DepthGuard::enter()?;
    let lang = lang_for_ext(ext)?;
    let mut out = format_one(lang, code, width.max(MIN_EMBED_WIDTH)).ok()?;
    // The parent owns the newline that closes the block.
    while out.ends_with('\n') {
        out.pop();
    }
    Some(out)
}

// --------------------------------- Formatters --------------------------------

/// JSON / JSONC / JSON5 via dprint-plugin-json. Key order, number precision
/// and (in jsonc mode) comments all survive, none of which was true of the
/// `serde_json::Value` round-trip this replaced.
fn format_json(content: &str, filename: &str, width: usize) -> Result<String, String> {
    let config = dprint_plugin_json::configuration::ConfigurationBuilder::new()
        .line_width(width as u32)
        .build();
    match dprint_plugin_json::format_text(Path::new(filename), content, &config) {
        Ok(Some(out)) => Ok(out),
        // `None` is "already formatted"; hand the input straight back.
        Ok(None) => Ok(content.to_string()),
        Err(e) => Err(format!("json: {e}")),
    }
}

/// CSS / SCSS / LESS / Sass via malva. Syntax flag picks the dialect; the
/// crate handles SCSS nesting, Sass indented syntax, and LESS extensions.
fn format_css(content: &str, syntax: malva::Syntax, width: usize) -> Result<String, String> {
    let mut opts = malva::config::FormatOptions::default();
    opts.layout.print_width = width;
    malva::format_text(content, syntax, &opts).map_err(|e| format!("css ({syntax:?}): {e}"))
}

/// HTML family + XML via markup_fmt. Every `<script>` / `<style>` /
/// `<template lang>` block and every code-carrying attribute comes back
/// through the callback, so embedded JS / TS / CSS is really formatted now
/// instead of being echoed verbatim.
fn format_markup(
    content: &str,
    language: markup_fmt::Language,
    width: usize,
) -> Result<String, String> {
    let mut opts = markup_fmt::config::FormatOptions::default();
    opts.layout.print_width = width;
    markup_fmt::format_text(content, language, &opts, |code, hints| {
        Ok::<_, anyhow::Error>(match format_embedded(hints.ext, code, hints.print_width) {
            Some(out) => Cow::Owned(out),
            None => Cow::Borrowed(code),
        })
    })
    .map_err(|e| format!("markup ({language:?}): {e}"))
}

/// JS / TS / JSX / TSX via dprint-plugin-typescript. Path drives the parser
/// mode (`.ts` vs `.tsx`); the in-memory filename never touches disk, it just
/// steers language detection inside the plugin. The external formatter reaches
/// the css / html / sql tagged templates that styled-components and lit-html
/// put in every modern component file.
fn format_ts(content: &str, filename: &str, width: usize) -> Result<String, String> {
    use dprint_plugin_typescript::configuration::ConfigurationBuilder;
    use dprint_plugin_typescript::{ExternalFormatter, FormatTextOptions};

    let config = ConfigurationBuilder::new().line_width(width as u32).build();
    let path = PathBuf::from(filename);
    // `move` matters: `ExternalFormatter` is `dyn Fn + 'static`, so a
    // by-reference capture of the local `width` would not live long enough.
    let embed: &ExternalFormatter = &move |tag, text, _cfg| Ok(format_embedded(tag, &text, width));
    let options = FormatTextOptions {
        path: &path,
        extension: None,
        text: content.to_string(),
        config: &config,
        external_formatter: Some(embed),
    };
    match dprint_plugin_typescript::format_text(options) {
        Ok(Some(out)) => Ok(out),
        Ok(None) => Ok(content.to_string()),
        Err(e) => Err(format!("typescript: {e}")),
    }
}

/// Markdown via dprint-plugin-markdown. Fenced code blocks are formatted in
/// their own language via the callback; a fence we have no formatter for, or
/// one that does not parse, is left byte-for-byte alone.
fn format_markdown(content: &str, width: usize) -> Result<String, String> {
    use dprint_plugin_markdown::configuration::ConfigurationBuilder;
    let config = ConfigurationBuilder::new().line_width(width as u32).build();
    let out = dprint_plugin_markdown::format_text(content, &config, |tag, code, line_width| {
        Ok(format_embedded(tag, code, line_width as usize))
    })
    .map_err(|e| format!("markdown: {e}"))?;
    Ok(out.unwrap_or_else(|| content.to_string()))
}

/// YAML via pretty_yaml. Multi-document streams keep their `---` separators;
/// comments are preserved (unlike a serde_yaml round-trip, which has no
/// representation for them).
fn format_yaml(content: &str, width: usize) -> Result<String, String> {
    let mut opts = pretty_yaml::config::FormatOptions::default();
    opts.layout.print_width = width;
    pretty_yaml::format_text(content, &opts).map_err(|e| format!("yaml: {e}"))
}

/// TOML via toml_edit. Preserves comments and overall structure.
fn format_toml(content: &str) -> Result<String, String> {
    let doc: toml_edit::DocumentMut = content
        .parse()
        .map_err(|e: toml_edit::TomlError| format!("toml parse: {e}"))?;
    Ok(doc.to_string())
}

/// SQL via sqlformat. Uppercases keywords; 2-space indent. `Generic` covers
/// the MySQL / PG / SQLite / SQL Server common subset - 0.5 added PostgreSql
/// and SQLServer dialects, but picking one here would break the other two and
/// nothing on the wire says which database a `.sql` file targets.
///
/// `..Default::default()` on purpose: the crate keeps adding knobs, and every
/// one of them defaults to the behaviour we already document.
fn format_sql(content: &str) -> String {
    use sqlformat::{format, Dialect, FormatOptions, Indent, QueryParams};
    let opts = FormatOptions {
        indent: Indent::Spaces(2),
        uppercase: Some(true),
        lines_between_queries: 2,
        dialect: Dialect::Generic,
        ..Default::default()
    };
    format(content, &QueryParams::None, &opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(lang: &str, src: &str) -> String {
        format_document(lang, src).unwrap_or_else(|e| panic!("{lang}: {e}"))
    }

    #[test]
    fn json_keeps_key_order() {
        // serde_json's default Map is a BTreeMap, so the old round-trip came
        // back sorted: reformatting a package.json reordered the whole file.
        let out = fmt("json", r#"{"z":1,"a":2,"m":3}"#);
        let (z, a) = (out.find("\"z\"").unwrap(), out.find("\"a\"").unwrap());
        assert!(z < a, "key order not preserved:\n{out}");
    }

    #[test]
    fn json_keeps_numbers_the_author_wrote() {
        // Measured against the old serde_json path, which stores i64 / u64 /
        // f64: u64::MAX survived it, so asserting on that alone proved
        // nothing. These are the values it actually rewrote - anything outside
        // i64/u64 range, and any decimal needing more than f64 precision:
        //
        //   123456789012345678901234567890 -> 1.2345678901234568e+29
        //   1.0000000000000001             -> 1.0
        //   -99999999999999999999          -> -1e+20
        for n in [
            "18446744073709551615",
            "123456789012345678901234567890",
            "1.0000000000000001",
            "-99999999999999999999",
        ] {
            let out = fmt("json", &format!(r#"{{"n":{n}}}"#));
            assert!(out.contains(n), "rewrote {n} as:\n{out}");
        }
    }

    #[test]
    fn jsonc_keeps_comments() {
        let out = fmt("jsonc", "{\n// keep me\n\"a\":1,\n}");
        assert!(out.contains("// keep me"), "comment dropped:\n{out}");
    }

    #[test]
    fn xml_does_not_split_a_gt_inside_an_attribute_or_comment() {
        let src = "<r><a title=\"x > y\"/><!-- p > q --></r>";
        let out = fmt("xml", src);
        assert!(out.contains("x > y"), "attribute value edited:\n{out}");
        assert!(out.contains("p > q"), "comment split:\n{out}");
    }

    #[test]
    fn crlf_and_bom_survive_a_round_trip() {
        let out = fmt("json", "\u{feff}{\"a\":  1}\r\n");
        assert!(out.starts_with('\u{feff}'), "BOM dropped");
        assert!(out.contains("\r\n"), "CRLF downgraded to LF");
        assert!(!out.contains("\r\r"), "CR doubled: {out:?}");
    }

    #[test]
    fn html_formats_its_embedded_script_and_style() {
        let out = fmt(
            "html",
            "<html><body><style>a{color:red}</style><script>const x={a:1,b:2}</script></body></html>",
        );
        assert!(out.contains("color: red"), "embedded css untouched:\n{out}");
        assert!(out.contains("const x = "), "embedded js untouched:\n{out}");
    }

    #[test]
    fn markdown_formats_a_fenced_code_block() {
        let out = fmt("markdown", "# t\n\n```json\n{\"a\":1}\n```\n");
        assert!(out.contains("\"a\": 1"), "fence body untouched:\n{out}");
    }

    #[test]
    fn tagged_template_css_is_formatted() {
        let out = fmt("typescript", "const s = css`a{color:red}`;\n");
        assert!(
            out.contains("color: red"),
            "tagged template untouched:\n{out}"
        );
    }

    #[test]
    fn unsupported_language_is_an_error_not_a_panic() {
        assert!(format_document("cobol", "IDENTIFICATION DIVISION.").is_err());
    }

    #[test]
    fn embedded_depth_is_bounded() {
        // Guard is a counter, not a marker: it has to come all the way back
        // down after a nested run or the next request formats nothing.
        let _ = fmt(
            "markdown",
            "```html\n<div><script>let a=1</script></div>\n```\n",
        );
        EMBED_DEPTH.with(|d| assert_eq!(d.get(), 0, "depth guard leaked"));
    }
}
