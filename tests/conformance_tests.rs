use std::{fs, path::Path};

use lumals::{diagnostics, formatting, parser, syntax::FileId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConformanceStatus {
    Pass,
    ExpectedDiagnostics,
    UnsupportedParseOnly,
}

#[test]
fn parser_and_formatter_conformance_fixtures_are_documented() {
    let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/conformance");
    let cases = [
        ("valid_core.luma", ConformanceStatus::Pass),
        ("valid_multidoc.luma", ConformanceStatus::Pass),
        (
            "invalid_policy.luma",
            ConformanceStatus::ExpectedDiagnostics,
        ),
        (
            "unsupported_eval.luma",
            ConformanceStatus::UnsupportedParseOnly,
        ),
    ];

    let mut report = Vec::new();
    for (index, (name, expected)) in cases.iter().enumerate() {
        let path = fixture_dir.join(name);
        let text = fs::read_to_string(&path).unwrap();
        let file_id = FileId(index as u32 + 1);
        let parsed = parser::parse_fallback(file_id, name, &text);
        let diagnostics = diagnostics::collect(&parsed);
        let documents = parsed.file.document_spans().len();

        match expected {
            ConformanceStatus::Pass => {
                assert!(diagnostics.is_empty(), "{name}: {diagnostics:?}");
                let formatted = formatting::format_text(file_id, name, parsed.backend, &text).text;
                let reparsed = parser::parse_fallback(file_id, name, &formatted);
                assert!(
                    diagnostics::collect(&reparsed).is_empty(),
                    "{name} formatted output should parse cleanly"
                );
                assert_eq!(
                    formatting::format_text(file_id, name, reparsed.backend, &formatted).text,
                    formatted,
                    "{name} formatting should be idempotent"
                );
                report.push(format!("{name}: pass ({documents} documents)"));
            }
            ConformanceStatus::ExpectedDiagnostics => {
                assert!(!diagnostics.is_empty(), "{name} should produce diagnostics");
                report.push(format!(
                    "{name}: expected diagnostics ({})",
                    diagnostics.len()
                ));
            }
            ConformanceStatus::UnsupportedParseOnly => {
                assert!(diagnostics.is_empty(), "{name}: syntax should still parse");
                report.push(format!(
                    "{name}: unsupported in v1 parse-only mode (evaluation not executed)"
                ));
            }
        }
    }

    assert_eq!(
        report,
        vec![
            "valid_core.luma: pass (1 documents)",
            "valid_multidoc.luma: pass (2 documents)",
            "invalid_policy.luma: expected diagnostics (5)",
            "unsupported_eval.luma: unsupported in v1 parse-only mode (evaluation not executed)",
        ]
    );
}
