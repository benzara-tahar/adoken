//! Syntax-free token coloring: a single-line lexer that maps keywords,
//! strings, numbers, comments, calls, types and punctuation to `syntax.*`
//! tokens. No grammars, no allocation beyond the token list, no state across
//! lines, so any visible row can be colored on its own in microseconds; the
//! diff paints plain text first and never waits on it (AC-14). Tree-sitter
//! highlighting can replace this per spec 0004 §9 without changing callers.

use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lang {
    Rust,
    /// TypeScript and JavaScript, incl. JSX/TSX.
    Script,
    CSharp,
    Python,
    Go,
    Json,
    Toml,
    Yaml,
    Shell,
    Markdown,
    Plain,
}

impl Lang {
    pub fn from_path(path: &str) -> Self {
        let name = path.rsplit('/').next().unwrap_or(path);
        let ext = name.rsplit_once('.').map_or("", |(_, ext)| ext);
        match ext.to_ascii_lowercase().as_str() {
            "rs" => Lang::Rust,
            "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "mts" | "cts" => Lang::Script,
            "cs" => Lang::CSharp,
            "py" | "pyi" => Lang::Python,
            "go" => Lang::Go,
            "json" | "jsonc" => Lang::Json,
            "toml" | "lock" => Lang::Toml,
            "yml" | "yaml" => Lang::Yaml,
            "sh" | "bash" | "zsh" => Lang::Shell,
            "md" | "markdown" => Lang::Markdown,
            _ => Lang::Plain,
        }
    }

    fn keywords(self) -> &'static [&'static str] {
        match self {
            Lang::Rust => &[
                "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else",
                "enum", "extern", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod",
                "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super",
                "trait", "type", "unsafe", "use", "where", "while", "yield",
            ],
            Lang::Script => &[
                "abstract",
                "as",
                "async",
                "await",
                "break",
                "case",
                "catch",
                "class",
                "const",
                "continue",
                "default",
                "delete",
                "do",
                "else",
                "enum",
                "export",
                "extends",
                "finally",
                "for",
                "from",
                "function",
                "get",
                "if",
                "implements",
                "import",
                "in",
                "instanceof",
                "interface",
                "keyof",
                "let",
                "new",
                "of",
                "private",
                "protected",
                "public",
                "readonly",
                "return",
                "set",
                "static",
                "super",
                "switch",
                "this",
                "throw",
                "try",
                "type",
                "typeof",
                "var",
                "void",
                "while",
                "yield",
            ],
            Lang::CSharp => &[
                "abstract",
                "async",
                "await",
                "base",
                "break",
                "case",
                "catch",
                "class",
                "const",
                "continue",
                "default",
                "do",
                "else",
                "enum",
                "finally",
                "for",
                "foreach",
                "get",
                "if",
                "in",
                "interface",
                "internal",
                "is",
                "namespace",
                "new",
                "out",
                "override",
                "private",
                "protected",
                "public",
                "readonly",
                "record",
                "return",
                "sealed",
                "set",
                "static",
                "struct",
                "switch",
                "this",
                "throw",
                "try",
                "using",
                "var",
                "virtual",
                "void",
                "while",
            ],
            Lang::Python => &[
                "and", "as", "assert", "async", "await", "break", "class", "continue", "def",
                "del", "elif", "else", "except", "finally", "for", "from", "global", "if",
                "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return",
                "try", "while", "with", "yield",
            ],
            Lang::Go => &[
                "break",
                "case",
                "chan",
                "const",
                "continue",
                "default",
                "defer",
                "else",
                "fallthrough",
                "for",
                "func",
                "go",
                "goto",
                "if",
                "import",
                "interface",
                "map",
                "package",
                "range",
                "return",
                "select",
                "struct",
                "switch",
                "type",
                "var",
            ],
            Lang::Shell => &[
                "case", "do", "done", "elif", "else", "esac", "export", "fi", "for", "function",
                "if", "in", "local", "return", "then", "while",
            ],
            Lang::Json | Lang::Toml | Lang::Yaml | Lang::Markdown | Lang::Plain => &[],
        }
    }

    fn line_comment(self) -> &'static [&'static str] {
        match self {
            Lang::Rust | Lang::Script | Lang::CSharp | Lang::Go | Lang::Json => &["//"],
            Lang::Python | Lang::Toml | Lang::Yaml | Lang::Shell => &["#"],
            Lang::Markdown | Lang::Plain => &[],
        }
    }

    fn c_like(self) -> bool {
        matches!(
            self,
            Lang::Rust | Lang::Script | Lang::CSharp | Lang::Go | Lang::Json
        )
    }
}

/// Maps 1:1 onto `syntax.*` tokens. Plain identifiers aren't emitted; the
/// caller paints them `syntax.variable`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    Keyword,
    String,
    Number,
    Constant,
    Comment,
    Function,
    Type,
    Property,
    Operator,
    Punctuation,
    Tag,
    Attribute,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub range: Range<usize>,
    pub kind: TokenKind,
}

const CONSTANTS: &[&str] = &[
    "true",
    "false",
    "null",
    "undefined",
    "None",
    "True",
    "False",
    "nil",
    "Some",
    "Ok",
    "Err",
];

/// Tokens of one line, sorted and non-overlapping.
pub fn tokenize(line: &str, lang: Lang) -> Vec<Token> {
    let mut out = Vec::new();
    if lang == Lang::Plain || line.is_empty() {
        return out;
    }
    let bytes = line.as_bytes();
    let trimmed_start = line.len() - line.trim_start().len();
    let rest = &line[trimmed_start..];

    if lang == Lang::Markdown {
        if rest.starts_with('#') {
            out.push(token(trimmed_start..line.len(), TokenKind::Keyword));
        } else if rest.starts_with("```") {
            out.push(token(trimmed_start..line.len(), TokenKind::Comment));
        }
        return out;
    }
    // Continuation lines of block comments / doc blocks: ` * text`, ` */`.
    if lang.c_like() && (rest == "*" || rest.starts_with("* ") || rest.starts_with("*/")) {
        out.push(token(trimmed_start..line.len(), TokenKind::Comment));
        return out;
    }
    if lang == Lang::Toml && rest.starts_with('[') {
        out.push(token(trimmed_start..line.len(), TokenKind::Type));
        return out;
    }

    let mut ix = 0;
    let mut prev_significant: Option<u8> = None;
    while ix < bytes.len() {
        let b = bytes[ix];
        let start = ix;
        if b.is_ascii_whitespace() {
            ix += 1;
            continue;
        }
        if lang
            .line_comment()
            .iter()
            .any(|marker| line[ix..].starts_with(marker))
        {
            out.push(token(ix..line.len(), TokenKind::Comment));
            break;
        }
        if lang.c_like() && line[ix..].starts_with("/*") {
            let end = line[ix + 2..]
                .find("*/")
                .map_or(line.len(), |e| ix + 2 + e + 2);
            out.push(token(ix..end, TokenKind::Comment));
            ix = end;
            continue;
        }
        if lang == Lang::Rust && line[ix..].starts_with("#[") || line[ix..].starts_with("#![") {
            let end = line[ix..].find(']').map_or(line.len(), |e| ix + e + 1);
            out.push(token(ix..end, TokenKind::Attribute));
            ix = end;
            continue;
        }
        if matches!(lang, Lang::Script | Lang::Python) && b == b'@' {
            ix += 1;
            while ix < bytes.len() && is_ident(bytes[ix]) {
                ix += 1;
            }
            out.push(token(start..ix, TokenKind::Attribute));
            continue;
        }
        let quote = match b {
            b'"' => true,
            b'`' => lang == Lang::Script || lang == Lang::Shell || lang == Lang::Go,
            b'\'' => lang != Lang::Rust || rust_char_literal_len(&bytes[ix..]).is_some(),
            _ => false,
        };
        if quote {
            let end = if lang == Lang::Rust && b == b'\'' {
                ix + rust_char_literal_len(&bytes[ix..]).unwrap_or(1)
            } else {
                string_end(bytes, ix)
            };
            let is_key = matches!(lang, Lang::Json | Lang::Yaml)
                && line[end..].trim_start().starts_with(':');
            out.push(token(
                ix..end,
                if is_key {
                    TokenKind::Property
                } else {
                    TokenKind::String
                },
            ));
            ix = end;
            prev_significant = Some(b'"');
            continue;
        }
        if b.is_ascii_digit() {
            while ix < bytes.len() && (is_ident(bytes[ix]) || bytes[ix] == b'.') {
                // `1..10`: stop before a range operator.
                if bytes[ix] == b'.' && bytes.get(ix + 1) == Some(&b'.') {
                    break;
                }
                ix += 1;
            }
            out.push(token(start..ix, TokenKind::Number));
            prev_significant = Some(b'0');
            continue;
        }
        if is_ident_start(b) {
            while ix < bytes.len() && is_ident(bytes[ix]) {
                ix += 1;
            }
            // Non-ASCII identifier characters: keep walking whole chars.
            while ix < bytes.len() && !bytes[ix].is_ascii() {
                ix += line[ix..].chars().next().map_or(1, char::len_utf8);
                while ix < bytes.len() && is_ident(bytes[ix]) {
                    ix += 1;
                }
            }
            let word = &line[start..ix];
            let next = line[ix..].trim_start().as_bytes().first().copied();
            let kind = if lang.keywords().contains(&word) {
                Some(TokenKind::Keyword)
            } else if CONSTANTS.contains(&word) {
                Some(TokenKind::Constant)
            } else if matches!(lang, Lang::Toml | Lang::Yaml)
                && start == trimmed_start
                && matches!(next, Some(b'=') | Some(b':'))
            {
                Some(TokenKind::Property)
            } else if next == Some(b'(') || (lang == Lang::Rust && next == Some(b'!')) {
                Some(TokenKind::Function)
            } else if prev_significant == Some(b'.') {
                Some(TokenKind::Property)
            } else if word.len() > 1
                && word
                    .bytes()
                    .all(|c| c.is_ascii_uppercase() || c == b'_' || c.is_ascii_digit())
                && word.bytes().any(|c| c.is_ascii_uppercase())
            {
                Some(TokenKind::Constant)
            } else if word.as_bytes()[0].is_ascii_uppercase() {
                Some(TokenKind::Type)
            } else if lang == Lang::Script && prev_significant == Some(b'<') {
                Some(TokenKind::Tag)
            } else {
                None
            };
            if let Some(kind) = kind {
                out.push(token(start..ix, kind));
            }
            prev_significant = Some(b'a');
            continue;
        }
        if b.is_ascii_punctuation() {
            ix += 1;
            let kind = match b {
                b'(' | b')' | b'{' | b'}' | b'[' | b']' | b',' | b';' | b'.' => {
                    TokenKind::Punctuation
                }
                _ => TokenKind::Operator,
            };
            match out.last_mut() {
                Some(last) if last.range.end == start && last.kind == kind => last.range.end = ix,
                _ => out.push(token(start..ix, kind)),
            }
            prev_significant = Some(b);
            continue;
        }
        // Any other non-ASCII character: skip it whole.
        ix += line[ix..].chars().next().map_or(1, char::len_utf8);
    }
    out
}

fn token(range: Range<usize>, kind: TokenKind) -> Token {
    Token { range, kind }
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b == b'$' || !b.is_ascii()
}

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

/// End (exclusive) of the string starting at `start`; unterminated strings
/// run to the end of the line.
fn string_end(bytes: &[u8], start: usize) -> usize {
    let quote = bytes[start];
    let mut ix = start + 1;
    while ix < bytes.len() {
        match bytes[ix] {
            b'\\' => ix += 2,
            b if b == quote => return ix + 1,
            _ => ix += 1,
        }
    }
    bytes.len()
}

/// `'a'`, `'\n'`, `'\u{1F600}'` or a multibyte char; `None` for lifetimes.
fn rust_char_literal_len(bytes: &[u8]) -> Option<usize> {
    if bytes.get(1) == Some(&b'\\') {
        return bytes[2..].iter().position(|b| *b == b'\'').map(|p| p + 3);
    }
    let width = match bytes.get(1)? {
        b if b.is_ascii() => 1,
        b if *b >= 0xF0 => 4,
        b if *b >= 0xE0 => 3,
        _ => 2,
    };
    (bytes.get(1 + width) == Some(&b'\'')).then_some(width + 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(line: &str, lang: Lang) -> Vec<(&str, TokenKind)> {
        tokenize(line, lang)
            .into_iter()
            .map(|t| (&line[t.range], t.kind))
            .collect()
    }

    #[test]
    fn rust_line() {
        use TokenKind::*;
        assert_eq!(
            kinds(
                "pub fn retry(n: u32) -> Result<(), Error> { // go",
                Lang::Rust
            ),
            [
                ("pub", Keyword),
                ("fn", Keyword),
                ("retry", Function),
                ("(", Punctuation),
                (":", Operator),
                (")", Punctuation),
                ("->", Operator),
                ("Result", Type),
                ("<", Operator),
                ("(),", Punctuation),
                ("Error", Type),
                (">", Operator),
                ("{", Punctuation),
                ("// go", Comment),
            ]
        );
    }

    #[test]
    fn rust_strings_chars_lifetimes_macros_attributes() {
        use TokenKind::*;
        let got = kinds(
            r#"#[inline] fn f<'a>(s: &'a str) { println!("x\"y"); let c = '\n'; }"#,
            Lang::Rust,
        );
        assert!(got.contains(&("#[inline]", Attribute)));
        assert!(got.contains(&("println", Function)));
        assert!(got.contains(&(r#""x\"y""#, String)));
        assert!(got.contains(&(r"'\n'", String)));
        assert!(!got.iter().any(|(text, _)| text.starts_with("'a")));
    }

    #[test]
    fn typescript_line() {
        use TokenKind::*;
        let got = kinds(
            "export const MAX_RETRIES = await client.fetch(`x`, 42); // hi",
            Lang::Script,
        );
        assert!(got.contains(&("export", Keyword)));
        assert!(got.contains(&("MAX_RETRIES", Constant)));
        assert!(got.contains(&("fetch", Function)));
        assert!(got.contains(&("`x`", String)));
        assert!(got.contains(&("42", Number)));
        assert!(got.contains(&("// hi", Comment)));
    }

    #[test]
    fn property_after_dot_and_json_keys() {
        use TokenKind::*;
        assert!(kinds("self.retries += 1;", Lang::Rust).contains(&("retries", Property)));
        assert_eq!(
            kinds(r#"  "name": "adoken","#, Lang::Json),
            [
                (r#""name""#, Property),
                (":", Operator),
                (r#""adoken""#, String),
                (",", Punctuation)
            ]
        );
    }

    #[test]
    fn doc_block_continuations_and_unterminated_strings() {
        assert_eq!(
            kinds("   * Retries the capture.", Lang::Script),
            [("* Retries the capture.", TokenKind::Comment)]
        );
        assert_eq!(
            kinds("let s = \"open", Lang::Rust).last(),
            Some(&("\"open", TokenKind::String))
        );
    }

    #[test]
    fn tokens_are_sorted_and_disjoint_on_char_boundaries() {
        let line = "let café = naïve(\"日本\", 'é') + x.y /* z */;";
        let tokens = tokenize(line, Lang::Rust);
        for pair in tokens.windows(2) {
            assert!(pair[0].range.end <= pair[1].range.start);
        }
        for t in &tokens {
            assert!(line.is_char_boundary(t.range.start) && line.is_char_boundary(t.range.end));
        }
    }

    #[test]
    fn language_from_path() {
        assert_eq!(Lang::from_path("src/a.rs"), Lang::Rust);
        assert_eq!(Lang::from_path("web/App.TSX"), Lang::Script);
        assert_eq!(Lang::from_path("Makefile"), Lang::Plain);
        assert_eq!(Lang::from_path("Cargo.lock"), Lang::Toml);
    }
}
