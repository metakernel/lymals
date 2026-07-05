use ropey::Rope;
use thiserror::Error;
use tower_lsp::lsp_types::{Position, Range};

use crate::syntax::SourceSpan;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineIndex {
    line_starts: Vec<usize>,
    text_len: usize,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PositionError {
    #[error("offset {offset} is out of bounds for text length {len}")]
    OffsetOutOfBounds { offset: usize, len: usize },
    #[error("offset {offset} is not on a UTF-8 character boundary")]
    OffsetNotCharBoundary { offset: usize },
    #[error("line {line} is out of bounds (max {max_line})")]
    LineOutOfBounds { line: u32, max_line: u32 },
    #[error("character {character} is out of bounds for line {line}")]
    CharacterOutOfBounds { line: u32, character: u32 },
    #[error("range start is after range end")]
    InvalidRange,
    #[error("span file id {actual:?} does not match expected {expected:?}")]
    MismatchedFileId {
        expected: crate::syntax::FileId,
        actual: crate::syntax::FileId,
    },
}

impl LineIndex {
    #[must_use]
    pub fn new(text: &str) -> Self {
        let mut line_starts = vec![0];

        for (offset, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(offset + 1);
            }
        }

        Self {
            line_starts,
            text_len: text.len(),
        }
    }

    #[must_use]
    pub fn from_rope(rope: &Rope) -> Self {
        Self::new(&rope.to_string())
    }

    #[must_use]
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    pub fn offset_to_position(&self, text: &str, offset: usize) -> Result<Position, PositionError> {
        if offset > self.text_len {
            return Err(PositionError::OffsetOutOfBounds {
                offset,
                len: self.text_len,
            });
        }

        if !text.is_char_boundary(offset) {
            return Err(PositionError::OffsetNotCharBoundary { offset });
        }

        let line = self.line_for_offset(offset);
        let line_start = self.line_starts[line];
        let utf16 = text[line_start..offset].encode_utf16().count() as u32;

        Ok(Position::new(line as u32, utf16))
    }

    pub fn position_to_offset(
        &self,
        text: &str,
        position: Position,
    ) -> Result<usize, PositionError> {
        let line = position.line as usize;
        if line >= self.line_count() {
            return Err(PositionError::LineOutOfBounds {
                line: position.line,
                max_line: self.line_count().saturating_sub(1) as u32,
            });
        }

        let start = self.line_starts[line];
        let end = self.line_text_end(text, line);
        let target = position.character;
        let mut utf16 = 0u32;
        let mut offset = start;

        for ch in text[start..end].chars() {
            if utf16 == target {
                return Ok(offset);
            }

            utf16 += ch.len_utf16() as u32;
            offset += ch.len_utf8();

            if utf16 == target {
                return Ok(offset);
            }

            if utf16 > target {
                return Err(PositionError::CharacterOutOfBounds {
                    line: position.line,
                    character: position.character,
                });
            }
        }

        if utf16 == target {
            Ok(end)
        } else {
            Err(PositionError::CharacterOutOfBounds {
                line: position.line,
                character: position.character,
            })
        }
    }

    pub fn range_to_span(
        &self,
        text: &str,
        file_id: crate::syntax::FileId,
        range: Range,
    ) -> Result<SourceSpan, PositionError> {
        let start = self.position_to_offset(text, range.start)?;
        let end = self.position_to_offset(text, range.end)?;

        if start > end {
            return Err(PositionError::InvalidRange);
        }

        Ok(SourceSpan::new(file_id, start, end))
    }

    pub fn span_to_range(&self, text: &str, span: SourceSpan) -> Result<Range, PositionError> {
        if span.start > span.end {
            return Err(PositionError::InvalidRange);
        }

        Ok(Range::new(
            self.offset_to_position(text, span.start)?,
            self.offset_to_position(text, span.end)?,
        ))
    }

    fn line_for_offset(&self, offset: usize) -> usize {
        self.line_starts.partition_point(|start| *start <= offset) - 1
    }

    fn line_text_end(&self, text: &str, line: usize) -> usize {
        let next_start = self
            .line_starts
            .get(line + 1)
            .copied()
            .unwrap_or(self.text_len);

        if next_start > 0 && text.as_bytes().get(next_start - 1) == Some(&b'\n') {
            next_start - 1
        } else {
            next_start
        }
    }
}
