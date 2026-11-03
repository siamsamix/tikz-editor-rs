//! Inclusive-exclusive span offsets into a source string.

/// A half-open `[from, to)` byte range in the source document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
pub struct Span {
    /// First byte inside the span.
    pub from: usize,
    /// One past the last byte inside the span.
    pub to: usize,
}

impl Span {
    /// Build a span.
    pub fn new(from: usize, to: usize) -> Span {
        Span { from, to }
    }

    /// Slice `source` with this span.
    pub fn slice<'a>(&self, source: &'a str) -> &'a str {
        &source[self.from..self.to]
    }

    /// Convert this span's byte offsets to UTF-16 code-unit offsets for
    /// parity output with the legacy TypeScript renderer.
    ///
    /// Legacy spans count UTF-16 code units; Rust byte offsets coincide only
    /// for ASCII. `byte_to_utf16` converts a byte offset into its equivalent
    /// UTF-16 code-unit offset by counting bytes before it and scaling
    /// non-ASCII characters to their UTF-16 unit count (1 for BMP, 2 for
    /// astral).
    pub fn to_utf16(&self, source: &str) -> Span {
        Span::new(
            byte_to_utf16(source, self.from),
            byte_to_utf16(source, self.to),
        )
    }

    /// Convert UTF-16 code-unit offsets back to byte offsets. Used when
    /// comparing against legacy dump output.
    pub fn from_utf16(source: &str, span: Span) -> Span {
        Span::new(
            utf16_to_byte(source, span.from),
            utf16_to_byte(source, span.to),
        )
    }
}

/// Convert a byte offset in `source` to a UTF-16 code-unit offset.
///
/// Non-ASCII characters occupy multiple bytes but one (BMP) or two (astral)
/// UTF-16 code units. We sum the UTF-16 unit counts of all characters whose
/// end byte offset is less than or equal to `byte_offset`. If `byte_offset`
/// falls inside a multi-byte character, earlier characters are counted but the
/// current one is not (rounding down to the character start).
pub fn byte_to_utf16(source: &str, byte_offset: usize) -> usize {
    let byte_offset = byte_offset.min(source.len());
    let mut total: usize = 0;
    let mut char_start = 0usize;
    for ch in source.chars() {
        let char_end = char_start + ch.len_utf8();
        if char_end > byte_offset {
            break;
        }
        total += ch.len_utf16();
        char_start = char_end;
    }
    total
}

/// Convert a UTF-16 code-unit offset back to a byte offset.
pub fn utf16_to_byte(source: &str, utf16_offset: usize) -> usize {
    let mut byte_idx = 0usize;
    let mut utf16_idx = 0usize;
    for ch in source.chars() {
        if utf16_idx >= utf16_offset {
            break;
        }
        utf16_idx += ch.len_utf16();
        byte_idx += ch.len_utf8();
    }
    byte_idx
}

/// Return the byte length of the character starting at `byte_idx`.
pub fn char_len_at(bytes: &[u8], byte_idx: usize) -> usize {
    if byte_idx >= bytes.len() {
        return 0;
    }
    let b = bytes[byte_idx];
    if b < 0x80 {
        1
    } else if b < 0xC0 {
        // Continuation byte - shouldn't be at start, but handle gracefully
        1
    } else if b < 0xE0 {
        2
    } else if b < 0xF0 {
        3
    } else {
        4
    }
}

#[cfg(test)]
mod span_tests {
    use super::*;

    #[test]
    fn ascii_spans_are_identity_under_utf16() {
        let source = "hello world";
        let span = Span::new(0, 5);
        assert_eq!(span.to_utf16(source), Span::new(0, 5));
        assert_eq!(Span::from_utf16(source, Span::new(0, 5)), Span::new(0, 5));
    }

    #[test]
    fn non_ascii_converts_byte_to_utf16() {
        // "Héllo": H(1) é(2 bytes, 1 utf16) l(1) l(1) o(1)
        let source = "Héllo";
        // Byte offset 2 is the start of 'é' (2 bytes: 0xC3 0xA9)
        // UTF-16 offset: H(1) + é(1) = 2
        let span = Span::new(2, 3); // 'é' in bytes
        assert_eq!(span.to_utf16(source), Span::new(1, 2));
    }

    #[test]
    fn astral_char_converts_correctly() {
        // "😀" is 4 bytes, 2 UTF-16 code units (surrogate pair)
        let source = "A😀B";
        // Byte offset 1 is start of 😀 (4 bytes)
        // UTF-16 offset: A(1) = 1
        let span = Span::new(1, 5); // '😀' in bytes (1+4=5)
        assert_eq!(span.to_utf16(source), Span::new(1, 3));
    }

    #[test]
    fn roundtrip_byte_to_utf16_to_byte() {
        let source = "Héllo 😀 world";
        // For every char-boundary byte offset, roundtrip through UTF-16
        // should recover the same byte offset.
        for (byte_offset, _) in source.char_indices() {
            let utf16 = byte_to_utf16(source, byte_offset);
            let back = utf16_to_byte(source, utf16);
            assert_eq!(
                back, byte_offset,
                "roundtrip failed at byte offset {byte_offset}"
            );
        }
        // Also test the end offset.
        let utf16 = byte_to_utf16(source, source.len());
        assert_eq!(utf16_to_byte(source, utf16), source.len());
        // Byte offsets inside multi-byte chars round down to the char start.
        assert_eq!(byte_to_utf16(source, 2), byte_to_utf16(source, 1)); // inside 'é'
        assert_eq!(byte_to_utf16(source, 8), byte_to_utf16(source, 7)); // inside '😀'
    }
}
