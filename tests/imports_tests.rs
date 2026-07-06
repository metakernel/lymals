use std::fs;

use tempfile::tempdir;
use tower_lsp::lsp_types::{Url, WorkspaceFolder};

use lymals::{
    config::LymalsConfig,
    imports::{
        ImportPolicyError, collect_resolution_diagnostics, resolve_guarded_import,
        resolve_import_graph,
    },
    syntax::FileId,
};

#[test]
fn resolves_local_import_graph_and_refreshes_from_disk() {
    let workspace = tempdir().unwrap();
    let root = workspace.path();
    fs::create_dir_all(root.join("shared")).unwrap();
    fs::write(
        root.join("main.lyma"),
        "@import \"./shared/a.lyma\" as a\nroot: true\n",
    )
    .unwrap();
    fs::write(root.join("shared/a.lyma"), "name: one\n").unwrap();

    let folders = folders(root);
    let main_uri = file_uri(root.join("main.lyma"));
    let main_text = fs::read_to_string(root.join("main.lyma")).unwrap();

    let first = resolve_import_graph(&main_uri, &main_text, &folders, &LymalsConfig::default());
    assert_eq!(first.diagnostics, Vec::new());
    assert_eq!(first.edges.len(), 1);
    assert!(
        first
            .files
            .iter()
            .any(|uri| uri.as_str().ends_with("shared/a.lyma"))
    );

    fs::write(
        root.join("shared/a.lyma"),
        "@include \"./b.lyma\"\nname: one\n",
    )
    .unwrap();
    fs::write(root.join("shared/b.lyma"), "name: two\n").unwrap();
    let refreshed = resolve_import_graph(&main_uri, &main_text, &folders, &LymalsConfig::default());
    assert_eq!(refreshed.diagnostics, Vec::new());
    assert_eq!(refreshed.edges.len(), 2);
    assert!(
        refreshed
            .files
            .iter()
            .any(|uri| uri.as_str().ends_with("shared/b.lyma"))
    );
}

#[test]
fn reports_missing_files_cycles_depth_and_edge_limits() {
    let workspace = tempdir().unwrap();
    let root = workspace.path();
    fs::write(root.join("main.lyma"), "@import \"./missing.lyma\"\n").unwrap();
    let folders = folders(root);
    let main_uri = file_uri(root.join("main.lyma"));
    let main_text = fs::read_to_string(root.join("main.lyma")).unwrap();

    let missing = resolve_import_graph(&main_uri, &main_text, &folders, &LymalsConfig::default());
    assert!(
        missing.diagnostics.iter().any(|diag| diag.code == "L019"),
        "{:?}",
        missing.diagnostics
    );

    fs::write(root.join("main.lyma"), "@import \"./a.lyma\"\n").unwrap();
    fs::write(root.join("a.lyma"), "@include \"./main.lyma\"\n").unwrap();
    let main_text = fs::read_to_string(root.join("main.lyma")).unwrap();
    let cycle = resolve_import_graph(&main_uri, &main_text, &folders, &LymalsConfig::default());
    assert!(cycle.diagnostics.iter().any(|diag| diag.code == "L021"));

    fs::write(root.join("main.lyma"), "@import \"./a.lyma\"\n").unwrap();
    fs::write(root.join("a.lyma"), "@include \"./b.lyma\"\n").unwrap();
    fs::write(root.join("b.lyma"), "name: b\n").unwrap();
    let limited = LymalsConfig {
        max_resolve_depth: 1,
        ..LymalsConfig::default()
    };
    let main_text = fs::read_to_string(root.join("main.lyma")).unwrap();
    let depth = resolve_import_graph(&main_uri, &main_text, &folders, &limited);
    assert!(depth.diagnostics.iter().any(|diag| diag.code == "L023"));

    let edge_limited = LymalsConfig {
        max_resolved_edges_per_file: 1,
        ..LymalsConfig::default()
    };
    let edges = resolve_import_graph(&main_uri, &main_text, &folders, &edge_limited);
    assert!(edges.diagnostics.iter().any(|diag| diag.code == "L024"));

    fs::write(root.join("big.lyma"), "x".repeat(32)).unwrap();
    fs::write(root.join("main.lyma"), "@import \"./big.lyma\"\n").unwrap();
    let oversized = resolve_import_graph(
        &main_uri,
        &fs::read_to_string(root.join("main.lyma")).unwrap(),
        &folders,
        &LymalsConfig {
            max_indexed_file_bytes: 8,
            ..LymalsConfig::default()
        },
    );
    assert!(
        oversized.diagnostics.iter().any(|diag| diag.code == "L020"),
        "{:?}",
        oversized.diagnostics
    );
}

#[test]
fn blocks_traversal_outside_roots_and_unsafe_schemes() {
    let workspace = tempdir().unwrap();
    let root = workspace.path();
    fs::write(root.join("main.lyma"), "root: true\n").unwrap();
    let folders = folders(root);
    let main_uri = file_uri(root.join("main.lyma"));

    assert_eq!(
        resolve_guarded_import(
            &main_uri,
            "../escape.lyma",
            &folders,
            &LymalsConfig::default()
        )
        .unwrap_err(),
        ImportPolicyError::ParentTraversal
    );
    assert_eq!(
        resolve_guarded_import(
            &main_uri,
            "https://example.test/pkg.lyma",
            &folders,
            &LymalsConfig::default()
        )
        .unwrap_err(),
        ImportPolicyError::DisallowedScheme("https".to_string())
    );
    assert_eq!(
        resolve_guarded_import(
            &main_uri,
            "pkg://registry/module",
            &folders,
            &LymalsConfig::default()
        )
        .unwrap_err(),
        ImportPolicyError::DisallowedScheme("pkg".to_string())
    );

    let outside = tempdir().unwrap();
    let outside_file = outside.path().join("outside.lyma");
    fs::write(&outside_file, "outside: true\n").unwrap();
    let outside_uri = file_uri(outside_file);
    let err = resolve_guarded_import(
        &main_uri,
        outside_uri.as_str(),
        &folders,
        &LymalsConfig::default(),
    )
    .unwrap_err();
    assert_eq!(err, ImportPolicyError::OutsideAllowedRoots);
}

#[test]
fn import_resolution_diagnostics_include_missing_targets() {
    let workspace = tempdir().unwrap();
    let root = workspace.path();
    let text = "@include \"./absent.lyma\"\nroot: true\n";
    fs::write(root.join("main.lyma"), text).unwrap();

    let diagnostics = collect_resolution_diagnostics(
        &file_uri(root.join("main.lyma")),
        text,
        FileId(0),
        &folders(root),
        &LymalsConfig::default(),
    );

    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "L019"),
        "{diagnostics:?}"
    );
}

#[test]
fn import_resolution_rejects_symlink_escape_when_supported() {
    let workspace = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let root = workspace.path();
    fs::write(root.join("main.lyma"), "@include \"./linked.lyma\"\n").unwrap();

    let outside_path = outside.path().join("escape.lyma");
    fs::write(&outside_path, "escape: true\n").unwrap();

    let link_path = root.join("linked.lyma");
    if try_create_file_symlink(&outside_path, &link_path).is_err() {
        return;
    }

    let diagnostics = collect_resolution_diagnostics(
        &file_uri(root.join("main.lyma")),
        &fs::read_to_string(root.join("main.lyma")).unwrap(),
        FileId(0),
        &folders(root),
        &LymalsConfig::default(),
    );

    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "L013"),
        "{diagnostics:?}"
    );
}

fn folders(path: &std::path::Path) -> Vec<WorkspaceFolder> {
    vec![WorkspaceFolder {
        uri: Url::from_directory_path(path).unwrap(),
        name: "workspace".to_string(),
    }]
}

fn file_uri(path: impl AsRef<std::path::Path>) -> Url {
    Url::from_file_path(path).unwrap()
}

#[cfg(unix)]
fn try_create_file_symlink(
    original: &std::path::Path,
    link: &std::path::Path,
) -> std::io::Result<()> {
    std::os::unix::fs::symlink(original, link)
}

#[cfg(windows)]
fn try_create_file_symlink(
    original: &std::path::Path,
    link: &std::path::Path,
) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(original, link)
}
