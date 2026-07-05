use std::fs;

use tempfile::tempdir;
use tower_lsp::lsp_types::{Url, WorkspaceFolder};

use lumals::{
    config::LumalsConfig,
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
        root.join("main.luma"),
        "@import \"./shared/a.luma\" as a\nroot: true\n",
    )
    .unwrap();
    fs::write(root.join("shared/a.luma"), "name: one\n").unwrap();

    let folders = folders(root);
    let main_uri = file_uri(root.join("main.luma"));
    let main_text = fs::read_to_string(root.join("main.luma")).unwrap();

    let first = resolve_import_graph(&main_uri, &main_text, &folders, &LumalsConfig::default());
    assert_eq!(first.diagnostics, Vec::new());
    assert_eq!(first.edges.len(), 1);
    assert!(
        first
            .files
            .iter()
            .any(|uri| uri.as_str().ends_with("shared/a.luma"))
    );

    fs::write(
        root.join("shared/a.luma"),
        "@include \"./b.luma\"\nname: one\n",
    )
    .unwrap();
    fs::write(root.join("shared/b.luma"), "name: two\n").unwrap();
    let refreshed = resolve_import_graph(&main_uri, &main_text, &folders, &LumalsConfig::default());
    assert_eq!(refreshed.diagnostics, Vec::new());
    assert_eq!(refreshed.edges.len(), 2);
    assert!(
        refreshed
            .files
            .iter()
            .any(|uri| uri.as_str().ends_with("shared/b.luma"))
    );
}

#[test]
fn reports_missing_files_cycles_depth_and_edge_limits() {
    let workspace = tempdir().unwrap();
    let root = workspace.path();
    fs::write(root.join("main.luma"), "@import \"./missing.luma\"\n").unwrap();
    let folders = folders(root);
    let main_uri = file_uri(root.join("main.luma"));
    let main_text = fs::read_to_string(root.join("main.luma")).unwrap();

    let missing = resolve_import_graph(&main_uri, &main_text, &folders, &LumalsConfig::default());
    assert!(
        missing.diagnostics.iter().any(|diag| diag.code == "L019"),
        "{:?}",
        missing.diagnostics
    );

    fs::write(root.join("main.luma"), "@import \"./a.luma\"\n").unwrap();
    fs::write(root.join("a.luma"), "@include \"./main.luma\"\n").unwrap();
    let main_text = fs::read_to_string(root.join("main.luma")).unwrap();
    let cycle = resolve_import_graph(&main_uri, &main_text, &folders, &LumalsConfig::default());
    assert!(cycle.diagnostics.iter().any(|diag| diag.code == "L021"));

    fs::write(root.join("main.luma"), "@import \"./a.luma\"\n").unwrap();
    fs::write(root.join("a.luma"), "@include \"./b.luma\"\n").unwrap();
    fs::write(root.join("b.luma"), "name: b\n").unwrap();
    let limited = LumalsConfig {
        max_resolve_depth: 1,
        ..LumalsConfig::default()
    };
    let main_text = fs::read_to_string(root.join("main.luma")).unwrap();
    let depth = resolve_import_graph(&main_uri, &main_text, &folders, &limited);
    assert!(depth.diagnostics.iter().any(|diag| diag.code == "L023"));

    let edge_limited = LumalsConfig {
        max_resolved_edges_per_file: 1,
        ..LumalsConfig::default()
    };
    let edges = resolve_import_graph(&main_uri, &main_text, &folders, &edge_limited);
    assert!(edges.diagnostics.iter().any(|diag| diag.code == "L024"));
}

#[test]
fn blocks_traversal_outside_roots_and_unsafe_schemes() {
    let workspace = tempdir().unwrap();
    let root = workspace.path();
    fs::write(root.join("main.luma"), "root: true\n").unwrap();
    let folders = folders(root);
    let main_uri = file_uri(root.join("main.luma"));

    assert_eq!(
        resolve_guarded_import(
            &main_uri,
            "../escape.luma",
            &folders,
            &LumalsConfig::default()
        )
        .unwrap_err(),
        ImportPolicyError::ParentTraversal
    );
    assert_eq!(
        resolve_guarded_import(
            &main_uri,
            "https://example.test/pkg.luma",
            &folders,
            &LumalsConfig::default()
        )
        .unwrap_err(),
        ImportPolicyError::DisallowedScheme("https".to_string())
    );

    let outside = tempdir().unwrap();
    let outside_file = outside.path().join("outside.luma");
    fs::write(&outside_file, "outside: true\n").unwrap();
    let outside_uri = file_uri(outside_file);
    let err = resolve_guarded_import(
        &main_uri,
        outside_uri.as_str(),
        &folders,
        &LumalsConfig::default(),
    )
    .unwrap_err();
    assert_eq!(err, ImportPolicyError::OutsideAllowedRoots);
}

#[test]
fn import_resolution_diagnostics_include_missing_targets() {
    let workspace = tempdir().unwrap();
    let root = workspace.path();
    let text = "@include \"./absent.luma\"\nroot: true\n";
    fs::write(root.join("main.luma"), text).unwrap();

    let diagnostics = collect_resolution_diagnostics(
        &file_uri(root.join("main.luma")),
        text,
        FileId(0),
        &folders(root),
        &LumalsConfig::default(),
    );

    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "L019"),
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
