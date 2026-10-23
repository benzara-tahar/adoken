use std::sync::Arc;

/// A blob's text with a line index. Cheap to clone (the source is shared);
/// line lookup is O(1) so rendering a visible row never scans the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Text {
    src: Arc<str>,
    /// Byte offset of each line start; one entry per line.
    starts: Vec<u32>,
}

impl Text {
    pub fn new(src: impl Into<Arc<str>>) -> Self {
        let src = src.into();
        let mut starts = Vec::with_capacity(src.len() / 32 + 1);
        if !src.is_empty() {
            starts.push(0);
            for (ix, byte) in src.bytes().enumerate() {
                if byte == b'\n' && ix + 1 < src.len() {
                    starts.push(ix as u32 + 1);
                }
            }
        }
        Self { src, starts }
    }

    pub fn source(&self) -> &Arc<str> {
        &self.src
    }

    /// Number of lines. A trailing newline doesn't start a new line; an empty
    /// text has zero lines.
    pub fn len(&self) -> usize {
        self.starts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.starts.is_empty()
    }

    /// Line `ix` (zero-based) without its terminator (`\n` or `\r\n`).
    pub fn line(&self, ix: usize) -> &str {
        let start = self.starts[ix] as usize;
        let end = self
            .starts
            .get(ix + 1)
            .map_or(self.src.len(), |next| *next as usize);
        let line = &self.src[start..end];
        let line = line.strip_suffix('\n').unwrap_or(line);
        line.strip_suffix('\r').unwrap_or(line)
    }

    pub fn lines(&self) -> impl ExactSizeIterator<Item = &str> + '_ {
        (0..self.len()).map(|ix| self.line(ix))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_handle_terminators() {
        let text = Text::new("a\r\nb\n\nc\n");
        assert_eq!(text.lines().collect::<Vec<_>>(), ["a", "b", "", "c"]);
        assert_eq!(Text::new("x").lines().collect::<Vec<_>>(), ["x"]);
        assert!(Text::new("").is_empty());
        assert_eq!(Text::new("\n").lines().collect::<Vec<_>>(), [""]);
    }
}
