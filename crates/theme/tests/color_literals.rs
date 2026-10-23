//! 0007 FR-1 / AC-1: colors come from theme tokens, so no crate other than
//! `crates/theme` may spell a color. Runs as part of `cargo test`.
//!
//! Scans every `.rs` file under `crates/` (except `crates/theme` and build
//! output) with a small Rust-aware lexer, so comments never match and the
//! checks below only look at code or at string contents respectively:
//!
//! - a string that is exactly a hex color (`"#F26B3A"`, `"#fff"`), or that
//!   contains a 6/8-digit hex run with a letter (`"border: #1a2b3c"`). Plain
//!   `"#1234"`/`"#123456"` inside text is left alone: that is how ADO writes
//!   work item and PR ids.
//! - `rgb(`/`rgba(`/`hsl(`/`hsla(` calls whose first argument is a number
//!   (gpui `rgb(0xff0000)`, `Color::rgba(0, 0, 0, 0)`), not conversions like
//!   `hsla(color)`.
//! - `Rgba`/`Hsla`/`Color` struct literals with numeric fields.
//! - gpui's named colors (`black()`, `white()`, `transparent_black()`, …).
//!
//! Exempt: `#[cfg(test)]` items and `tests/` directories. Test fixtures check
//! conversions and contrast math against known values; they never paint the
//! shipping UI, and forcing them through tokens would hide what they assert.

use std::path::{Path, PathBuf};

#[test]
fn no_color_literals_outside_the_theme_crate() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate lives in crates/")
        .to_path_buf();
    let mut files = Vec::new();
    collect_sources(&crates, &crates.join("theme"), &mut files);
    assert!(
        files.iter().any(|f| f.starts_with(crates.join("ui"))),
        "scanner found no crates/ui sources; is the walk rooted correctly?"
    );

    let root = crates.parent().unwrap_or(&crates);
    let mut findings = Vec::new();
    for file in &files {
        let source = std::fs::read_to_string(file).expect("readable source file");
        for finding in scan(&source) {
            let path = file.strip_prefix(root).unwrap_or(file);
            findings.push(format!(
                "{}:{}: {}",
                path.display(),
                finding.line,
                finding.what
            ));
        }
    }
    assert!(
        findings.is_empty(),
        "color literals outside crates/theme (use theme tokens, docs/design/tokens.md):\n{}",
        findings.join("\n")
    );
}

fn collect_sources(dir: &Path, skip: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .expect("readable directory")
        .map(|entry| entry.expect("readable entry").path())
        .collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if path.is_dir() {
            let excluded =
                path == skip || name.starts_with('.') || matches!(name, "target" | "tests");
            if !excluded {
                collect_sources(&path, skip, out);
            }
        } else if name.ends_with(".rs") {
            out.push(path);
        }
    }
}

#[derive(Debug, PartialEq)]
struct Finding {
    line: usize,
    what: String,
}

/// Source split into code (comments and string contents blanked, offsets and
/// newlines preserved) and the string literals with their start offsets.
struct Lexed {
    code: Vec<u8>,
    strings: Vec<(usize, String)>,
}

fn lex(source: &str) -> Lexed {
    let src = source.as_bytes();
    let mut code = src.to_vec();
    let mut strings = Vec::new();
    let blank = |code: &mut Vec<u8>, from: usize, to: usize| {
        for byte in &mut code[from..to] {
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
    };
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut i = 0;
    while i < src.len() {
        match src[i] {
            b'/' if src.get(i + 1) == Some(&b'/') => {
                let end = src[i..]
                    .iter()
                    .position(|&b| b == b'\n')
                    .map_or(src.len(), |n| i + n);
                blank(&mut code, i, end);
                i = end;
            }
            b'/' if src.get(i + 1) == Some(&b'*') => {
                let start = i;
                let mut depth = 0;
                while i < src.len() {
                    if src[i..].starts_with(b"/*") {
                        depth += 1;
                        i += 2;
                    } else if src[i..].starts_with(b"*/") {
                        depth -= 1;
                        i += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
                blank(&mut code, start, i);
            }
            b'r' if (i == 0 || !is_ident(src[i - 1]) || src[i - 1] == b'b')
                && matches!(src.get(i + 1), Some(b'"' | b'#')) =>
            {
                let hashes = src[i + 1..].iter().take_while(|&&b| b == b'#').count();
                let open = i + 1 + hashes;
                if src.get(open) != Some(&b'"') {
                    i += 1;
                    continue;
                }
                let mut close = Vec::with_capacity(hashes + 1);
                close.push(b'"');
                close.extend(std::iter::repeat_n(b'#', hashes));
                let body = open + 1;
                let end = src[body..]
                    .windows(close.len())
                    .position(|w| w == close.as_slice())
                    .map_or(src.len(), |n| body + n);
                strings.push((body, source[body..end].to_owned()));
                blank(&mut code, body, end);
                i = (end + close.len()).min(src.len());
            }
            b'"' => {
                let body = i + 1;
                let mut j = body;
                while j < src.len() && src[j] != b'"' {
                    j += if src[j] == b'\\' { 2 } else { 1 };
                }
                let end = j.min(src.len());
                strings.push((body, source[body..end].to_owned()));
                blank(&mut code, body, end);
                i = end + 1;
            }
            b'\'' => {
                // Char literal ('x', '\n', '"') or lifetime ('a).
                if src.get(i + 1) == Some(&b'\\') {
                    let end = src[i + 2..]
                        .iter()
                        .position(|&b| b == b'\'')
                        .map_or(src.len(), |n| i + 2 + n);
                    blank(&mut code, i + 1, end);
                    i = end + 1;
                } else {
                    let width = source[i + 1..].chars().next().map_or(1, char::len_utf8);
                    if src.get(i + 1 + width) == Some(&b'\'') {
                        blank(&mut code, i + 1, i + 1 + width);
                        i += width + 2;
                    } else {
                        i += 1;
                    }
                }
            }
            _ => i += 1,
        }
    }
    Lexed { code, strings }
}

/// Byte ranges of `#[cfg(test)]` items (attribute through closing `}` or `;`).
fn test_only_ranges(code: &[u8]) -> Vec<(usize, usize)> {
    const ATTR: &[u8] = b"#[cfg(test)]";
    let mut ranges = Vec::new();
    let mut from = 0;
    while let Some(start) = find(code, ATTR, from) {
        let mut i = start + ATTR.len();
        let mut depth = 0usize;
        let mut end = code.len();
        while i < code.len() {
            match code[i] {
                b'{' => depth += 1,
                b'}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        end = i + 1;
                        break;
                    }
                }
                b';' if depth == 0 => {
                    end = i + 1;
                    break;
                }
                _ => {}
            }
            i += 1;
        }
        ranges.push((start, end));
        from = end;
    }
    ranges
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack
        .get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|n| from + n)
}

fn scan(source: &str) -> Vec<Finding> {
    let Lexed { code, strings } = lex(source);
    let exempt = test_only_ranges(&code);
    let exempt = |offset: usize| exempt.iter().any(|&(s, e)| (s..e).contains(&offset));
    let line = |offset: usize| 1 + code[..offset].iter().filter(|&&b| b == b'\n').count();
    let mut findings = Vec::new();

    for (offset, text) in &strings {
        if !exempt(*offset) && string_has_hex_color(text) {
            findings.push(Finding {
                line: line(*offset),
                what: format!("hex color in string \"{text}\""),
            });
        }
    }

    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let skip_ws = |mut i: usize| {
        while code.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        i
    };
    let starts_number = |i: usize| match code.get(i) {
        Some(b) if b.is_ascii_digit() => true,
        Some(b'-' | b'.') => code.get(i + 1).is_some_and(u8::is_ascii_digit),
        _ => false,
    };
    let mut i = 0;
    while i < code.len() {
        if !is_ident(code[i]) || (i > 0 && is_ident(code[i - 1])) {
            i += 1;
            continue;
        }
        let start = i;
        while i < code.len() && is_ident(code[i]) {
            i += 1;
        }
        if exempt(start) {
            continue;
        }
        let ident = &source[start..i];
        let after = skip_ws(i);
        let what = match ident {
            "rgb" | "rgba" | "hsl" | "hsla"
                if code.get(after) == Some(&b'(') && starts_number(skip_ws(after + 1)) =>
            {
                Some(format!("{ident}(…) with numeric channels"))
            }
            "Rgba" | "Hsla" | "Color" if code.get(after) == Some(&b'{') => {
                let field = skip_ws(after + 1);
                let mut j = field;
                while code.get(j).is_some_and(|&b| is_ident(b)) {
                    j += 1;
                }
                let colon = skip_ws(j);
                (j > field
                    && code.get(colon) == Some(&b':')
                    && code.get(colon + 1) != Some(&b':')
                    && starts_number(skip_ws(colon + 1)))
                .then(|| format!("{ident} {{ … }} with numeric channels"))
            }
            "black" | "white" | "red" | "green" | "blue" | "yellow" | "transparent_black"
            | "transparent_white" | "opaque_grey"
                if code.get(after) == Some(&b'(')
                    && code.get(skip_ws(after + 1)) == Some(&b')')
                    && !is_method_or_definition(&code, start) =>
            {
                Some(format!("gpui named color {ident}()"))
            }
            _ => None,
        };
        if let Some(what) = what {
            findings.push(Finding {
                line: line(start),
                what,
            });
        }
    }
    findings.sort_by_key(|finding| finding.line);
    findings
}

/// `x.red()` is a channel accessor and `fn white()` a definition, not a color.
fn is_method_or_definition(code: &[u8], ident_start: usize) -> bool {
    let before = code[..ident_start].trim_ascii_end();
    before.ends_with(b".") || before.ends_with(b"fn")
}

fn string_has_hex_color(text: &str) -> bool {
    let is_hex_color = |run: &str| matches!(run.len(), 3 | 4 | 6 | 8);
    let trimmed = text.trim();
    if let Some(hex) = trimmed.strip_prefix('#')
        && is_hex_color(hex)
        && hex.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return true;
    }
    let bytes = text.as_bytes();
    bytes.iter().enumerate().any(|(i, &b)| {
        if b != b'#' {
            return false;
        }
        let run = bytes[i + 1..]
            .iter()
            .take_while(|b| b.is_ascii_alphanumeric())
            .count();
        let run = &text[i + 1..i + 1 + run];
        matches!(run.len(), 6 | 8)
            && run.bytes().all(|b| b.is_ascii_hexdigit())
            && run.bytes().any(|b| b.is_ascii_alphabetic())
    })
}

#[cfg(test)]
mod scanner {
    use super::scan;

    fn lines(source: &str) -> Vec<usize> {
        scan(source).into_iter().map(|f| f.line).collect()
    }

    #[test]
    fn flags_hex_strings_and_numeric_color_constructors() {
        let source = r##"
let a = Color::hex("#F26B3A");
let b = rgb(0xff00aa);
let c = Color::rgba(0, 0, 0, 0);
let d = Hsla {
    h: 0.5, s: 1.0, l: 0.5, a: 1.0 };
let e = gpui::black();
let f = format!("border: #1a2b3c");
let g = hsla(-0.1, 0.2, 0.3, 1.0);
let h = r#"#fff"#;
"##;
        assert_eq!(lines(source), vec![2, 3, 4, 5, 7, 8, 9, 10]);
    }

    #[test]
    fn ignores_comments_conversions_ids_and_accessors() {
        let source = r##"
// bg.app = "#0E0F13"
/* rgb(0, 0, 0) /* nested */ white() */
/// Paints `#FF8A5F` on hover.
let a = hsla(color);
let b = Rgba { r: f32::from(color.r) / 255.0, g, b, a };
let c = "Fix crash in PR #4512 (work item #123456)";
let d = color.red();
fn white() {}
let e = '"'; let f = "#[derive(Debug)]";
let g: &'static str = "https://dev.azure.com";
"##;
        assert_eq!(scan(source), vec![]);
    }

    #[test]
    fn exempts_cfg_test_items_but_not_code_after_them() {
        let source = "
#[cfg(test)]
mod tests {
    fn fixture() { let _ = (\"#ffffff\", rgb(0x000000)); }
}
fn paint() { let _ = rgb(0x123456); }
";
        assert_eq!(lines(source), vec![6]);
    }
}
