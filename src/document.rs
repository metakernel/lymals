use std::collections::HashMap;
use std::sync::Arc;

use ropey::Rope;
use thiserror::Error;
use tower_lsp::lsp_types::{Position, Range, Url};

use crate::parser::{self, ParsedDocument};
use crate::position::{LineIndex, PositionError};
use crate::syntax::{FileId, SourceSpan};

#[derive(Debug, Clone)]
pub struct Document {
    uri: Url,
    version: i32,
    file_id: FileId,
    text: Rope,
    line_index: LineIndex,
    parse_cache: Option<Arc<ParsedDocument>>,
    dirty: bool,
}

#[derive(Debug, Clone)]
pub struct DocumentSnapshot {
    pub uri: Url,
    pub file_id: FileId,
    pub text: String,
}

#[derive(Debug, Default)]
pub struct DocumentStore {
    documents: HashMap<Url, Document>,
    next_file_id: u32,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DocumentStoreError {
    #[error("document not found: {0}")]
    NotFound(Url),
}

impl Document {
    #[must_use]
    pub fn new(uri: Url, version: i32, text: impl AsRef<str>, file_id: FileId) -> Self {
        let rope = Rope::from_str(text.as_ref());
        let line_index = LineIndex::from_rope(&rope);

        Self {
            uri,
            version,
            file_id,
            text: rope,
            line_index,
            parse_cache: None,
            dirty: true,
        }
    }

    #[must_use]
    pub fn uri(&self) -> &Url {
        &self.uri
    }

    #[must_use]
    pub fn version(&self) -> i32 {
        self.version
    }

    #[must_use]
    pub fn file_id(&self) -> FileId {
        self.file_id
    }

    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    #[must_use]
    pub fn parse_cache(&self) -> Option<Arc<ParsedDocument>> {
        self.parse_cache.clone()
    }

    #[must_use]
    pub fn line_index(&self) -> &LineIndex {
        &self.line_index
    }

    #[must_use]
    pub fn text(&self) -> String {
        self.text.to_string()
    }

    #[must_use]
    pub fn snapshot(&self) -> DocumentSnapshot {
        DocumentSnapshot {
            uri: self.uri.clone(),
            file_id: self.file_id,
            text: self.text(),
        }
    }

    pub fn update(&mut self, version: i32, text: impl AsRef<str>) {
        self.version = version;
        self.text = Rope::from_str(text.as_ref());
        self.line_index = LineIndex::from_rope(&self.text);
        self.parse_cache = None;
        self.dirty = true;
    }

    #[must_use]
    pub fn parsed(&mut self) -> Arc<ParsedDocument> {
        if self.dirty || self.parse_cache.is_none() {
            let text = self.text();
            let parsed = Arc::new(parser::parse(self.file_id, self.uri.as_str(), &text));
            self.parse_cache = Some(parsed.clone());
            self.dirty = false;
        }

        self.parse_cache
            .as_ref()
            .cloned()
            .expect("parse cache populated")
    }

    pub fn position_to_offset(&self, position: Position) -> Result<usize, PositionError> {
        let text = self.text();
        self.line_index.position_to_offset(&text, position)
    }

    pub fn offset_to_position(&self, offset: usize) -> Result<Position, PositionError> {
        let text = self.text();
        self.line_index.offset_to_position(&text, offset)
    }

    pub fn range_to_span(&self, range: Range) -> Result<SourceSpan, PositionError> {
        let text = self.text();
        self.line_index.range_to_span(&text, self.file_id, range)
    }

    pub fn span_to_range(&self, span: SourceSpan) -> Result<Range, PositionError> {
        if span.file_id != self.file_id {
            return Err(PositionError::MismatchedFileId {
                expected: self.file_id,
                actual: span.file_id,
            });
        }

        let text = self.text();
        self.line_index.span_to_range(&text, span)
    }
}

impl DocumentStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open(&mut self, uri: Url, version: i32, text: impl AsRef<str>) -> FileId {
        let file_id = self
            .documents
            .get(&uri)
            .map(Document::file_id)
            .unwrap_or_else(|| self.allocate_file_id());

        let document = Document::new(uri.clone(), version, text, file_id);
        self.documents.insert(uri, document);
        file_id
    }

    pub fn update(
        &mut self,
        uri: &Url,
        version: i32,
        text: impl AsRef<str>,
    ) -> Result<FileId, DocumentStoreError> {
        let document = self
            .documents
            .get_mut(uri)
            .ok_or_else(|| DocumentStoreError::NotFound(uri.clone()))?;
        document.update(version, text);
        Ok(document.file_id())
    }

    pub fn close(&mut self, uri: &Url) -> Option<Document> {
        self.documents.remove(uri)
    }

    #[must_use]
    pub fn get(&self, uri: &Url) -> Option<&Document> {
        self.documents.get(uri)
    }

    pub fn get_mut(&mut self, uri: &Url) -> Option<&mut Document> {
        self.documents.get_mut(uri)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.documents.len()
    }

    #[must_use]
    pub fn snapshots(&self) -> Vec<DocumentSnapshot> {
        self.documents.values().map(Document::snapshot).collect()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }

    fn allocate_file_id(&mut self) -> FileId {
        let file_id = FileId(self.next_file_id);
        self.next_file_id = self.next_file_id.saturating_add(1);
        file_id
    }
}
