use std::{fs, time::Instant};

use tempfile::tempdir;
use tower_lsp::lsp_types::{Url, WorkspaceFolder};

use lumals::{config::LumalsConfig, index::WorkspaceIndex, parser, syntax::FileId};

fn main() {
    let mut text = String::from("root:\n");
    for index in 0..10_000 {
        text.push_str(&format!("  key_{index}: value_{index}\n"));
    }

    let parse_started = Instant::now();
    let parsed = parser::parse_fallback(FileId(1), "bench.luma", &text);
    println!(
        "parse diagnostics={} elapsed={:?}",
        parsed.diagnostics.len(),
        parse_started.elapsed()
    );

    let temp = tempdir().expect("tempdir");
    for index in 0..250 {
        fs::write(
            temp.path().join(format!("file_{index}.luma")),
            format!("key_{index}: true\n"),
        )
        .expect("write fixture");
    }
    let folders = vec![WorkspaceFolder {
        uri: Url::from_directory_path(temp.path()).expect("workspace uri"),
        name: "bench".to_string(),
    }];

    let index_started = Instant::now();
    let index = WorkspaceIndex::load(&[], &folders, &LumalsConfig::default());
    println!(
        "indexed={} elapsed={:?}",
        index.documents().len(),
        index_started.elapsed()
    );
}
