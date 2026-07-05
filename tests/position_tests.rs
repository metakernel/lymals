use tower_lsp::lsp_types::{Position, Range, Url};

use lumals::document::{Document, DocumentStore, DocumentStoreError};
use lumals::position::PositionError;
use lumals::syntax::{FileId, SourceSpan};

fn document(text: &str) -> Document {
    Document::new(Url::parse("file:///test.luma").unwrap(), 1, text, FileId(7))
}

#[test]
fn ascii_round_trips_offsets_positions_and_ranges() {
    let doc = document("alpha\nbeta");

    assert_eq!(doc.offset_to_position(0).unwrap(), Position::new(0, 0));
    assert_eq!(doc.offset_to_position(3).unwrap(), Position::new(0, 3));
    assert_eq!(doc.offset_to_position(6).unwrap(), Position::new(1, 0));
    assert_eq!(doc.position_to_offset(Position::new(1, 2)).unwrap(), 8);

    let span = doc
        .range_to_span(Range::new(Position::new(0, 1), Position::new(1, 2)))
        .unwrap();
    assert_eq!(span, SourceSpan::new(FileId(7), 1, 8));
    assert_eq!(
        doc.span_to_range(span).unwrap(),
        Range::new(Position::new(0, 1), Position::new(1, 2))
    );
}

#[test]
fn crlf_positions_treat_newline_as_line_boundary() {
    let doc = document("a\r\nβ\r\n");

    assert_eq!(doc.offset_to_position(2).unwrap(), Position::new(0, 2));
    assert_eq!(doc.position_to_offset(Position::new(1, 1)).unwrap(), 5);
    assert_eq!(doc.position_to_offset(Position::new(1, 2)).unwrap(), 6);
}

#[test]
fn unicode_and_emoji_use_utf16_columns() {
    let doc = document("é🙂z");

    assert_eq!(doc.offset_to_position(0).unwrap(), Position::new(0, 0));
    assert_eq!(
        doc.offset_to_position("é".len()).unwrap(),
        Position::new(0, 1)
    );
    assert_eq!(
        doc.offset_to_position("é🙂".len()).unwrap(),
        Position::new(0, 3)
    );
    assert_eq!(
        doc.position_to_offset(Position::new(0, 1)).unwrap(),
        "é".len()
    );
    assert_eq!(
        doc.position_to_offset(Position::new(0, 3)).unwrap(),
        "é🙂".len()
    );
}

#[test]
fn eof_positions_are_supported() {
    let doc = document("x\n🙂");
    let eof = "x\n🙂".len();

    assert_eq!(doc.offset_to_position(eof).unwrap(), Position::new(1, 2));
    assert_eq!(doc.position_to_offset(Position::new(1, 2)).unwrap(), eof);
}

#[test]
fn invalid_ranges_and_offsets_fail_safely() {
    let doc = document("🙂");

    assert_eq!(
        doc.offset_to_position(1).unwrap_err(),
        PositionError::OffsetNotCharBoundary { offset: 1 }
    );
    assert_eq!(
        doc.position_to_offset(Position::new(0, 1)).unwrap_err(),
        PositionError::CharacterOutOfBounds {
            line: 0,
            character: 1,
        }
    );
    assert_eq!(
        doc.position_to_offset(Position::new(1, 0)).unwrap_err(),
        PositionError::LineOutOfBounds {
            line: 1,
            max_line: 0,
        }
    );
    assert_eq!(
        doc.range_to_span(Range::new(Position::new(0, 2), Position::new(0, 0)))
            .unwrap_err(),
        PositionError::InvalidRange
    );
    assert_eq!(
        doc.span_to_range(SourceSpan::new(FileId(99), 0, 0))
            .unwrap_err(),
        PositionError::MismatchedFileId {
            expected: FileId(7),
            actual: FileId(99),
        }
    );
}

#[test]
fn document_store_tracks_version_dirty_and_parse_cache() {
    let uri = Url::parse("file:///store.luma").unwrap();
    let mut store = DocumentStore::new();
    let file_id = store.open(uri.clone(), 1, "key: value");

    let doc = store.get(&uri).unwrap();
    assert_eq!(doc.file_id(), file_id);
    assert_eq!(doc.version(), 1);
    assert!(doc.is_dirty());
    assert!(doc.parse_cache().is_none());

    let first_backend = store.get_mut(&uri).unwrap().parsed().backend;
    let doc = store.get(&uri).unwrap();
    assert!(!doc.is_dirty());
    assert_eq!(doc.parse_cache().unwrap().backend, first_backend);

    store.update(&uri, 2, "other: value").unwrap();
    let doc = store.get(&uri).unwrap();
    assert_eq!(doc.version(), 2);
    assert!(doc.is_dirty());
    assert!(doc.parse_cache().is_none());

    assert!(matches!(
        store.update(&Url::parse("file:///missing.luma").unwrap(), 1, ""),
        Err(DocumentStoreError::NotFound(_))
    ));
}
