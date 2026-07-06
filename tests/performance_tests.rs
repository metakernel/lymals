use std::{fs, time::Instant};

use tempfile::tempdir;
use tower_lsp::lsp_types::{Url, WorkspaceFolder};

use lymals::{config::LymalsConfig, index::WorkspaceIndex, parser, syntax::FileId};

#[test]
fn large_files_and_many_workspace_files_establish_baseline_without_panic() {
    let temp = tempdir().unwrap();
    let root = temp.path();
    let workspace = vec![WorkspaceFolder {
        uri: Url::from_directory_path(root).unwrap(),
        name: "perf".to_string(),
    }];

    let mut large = String::from("root:\n");
    for index in 0..5_000 {
        large.push_str(&format!("  key_{index}: value_{index}\n"));
    }
    let started = Instant::now();
    let parsed = parser::parse_fallback(FileId(1), "large.lyma", &large);
    assert!(parsed.diagnostics.is_empty());
    assert!(
        started.elapsed().as_secs() < 5,
        "large parse baseline exceeded"
    );

    for index in 0..120 {
        fs::write(
            root.join(format!("file_{index}.lyma")),
            format!("item_{index}: true\n"),
        )
        .unwrap();
    }
    fs::write(root.join("ignore.txt"), "not lyma\n").unwrap();

    let started = Instant::now();
    let index = WorkspaceIndex::load(&[], &workspace, &LymalsConfig::default());
    assert_eq!(index.documents().len(), 120);
    assert!(
        started.elapsed().as_secs() < 5,
        "workspace index baseline exceeded"
    );
}

#[test]
fn unicode_heavy_file_stays_under_baseline_and_parses() {
    let mut text = String::from("emoji:\n");
    for index in 0..1_000 {
        text.push_str(&format!("  item_{index}: \"🚀 Привет こんにちは مرحبا\"\n"));
    }

    let started = Instant::now();
    let parsed = parser::parse_fallback(FileId(2), "unicode.lyma", &text);

    assert!(parsed.diagnostics.is_empty());
    assert!(
        started.elapsed().as_secs() < 5,
        "unicode parse baseline exceeded"
    );
}
