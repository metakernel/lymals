# Development notes

## Dependency choices

- `tower-lsp`: primary LSP transport/server framework. It already re-exports the `lsp_types` surface we need, so we avoid a separate direct `lsp-types` dependency unless version pinning becomes necessary.
- `tokio`: async runtime for stdio LSP serving, background indexing, cancellation, and future task coordination.
- `serde` + `serde_json`: config, protocol-adjacent JSON handling, fixtures, and test payloads.
- `tracing` + `tracing-subscriber`: structured logs for editor/LSP debugging with env-configurable filtering.
- `clap`: small, standard CLI layer for binary flags like explicit stdio mode and future debug/config commands.
- `ropey`: efficient text storage plus line/offset mapping for LSP ranges, edits, and diagnostics.
- `parking_lot`: chosen over `dashmap` for near-term shared state because current architecture needs predictable locks around document/workspace state more than a concurrent map API. Revisit `dashmap` only if profiling shows real contention on keyed state.
- `thiserror`: typed internal errors for library boundaries and subsystem-specific failures.
- `anyhow`: ergonomic top-level/application error aggregation at binary and task boundaries.
- `insta`: snapshot coverage for diagnostics, formatting, and protocol payloads.
- `pretty_assertions`: readable structural diffs in unit/integration tests.
- `tempfile`: isolated workspace/file-fixture tests without polluting the repo.
- `proptest`: parser/formatter invariants and edge-case generation where example-based tests are too narrow.
- `similar`: text diff support for formatter and snapshot-adjacent assertions when we want explicit diff output beyond plain equality checks.

## Deferred or special cases

- `lyma` remains optional and feature-gated because v1 stays parse-only by default, with upstream integration behind the local syntax facade.
- `miette` remains in the crate for rich human-facing diagnostics/CLI error presentation even though it was not part of the narrower preferred set.
- We intentionally do **not** add a direct `dashmap` dependency yet; `parking_lot` covers the current root-bounded workspace/document-state plan with less API commitment.

## Common commands

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test
cargo test --all-features
cargo test --test lsp_harness
cargo bench --bench parse_index
cargo doc --workspace --all-features --no-deps
cargo deny check
```

Use `UPDATE_GOLDENS=1 cargo test --test parser_tests` only when intentionally refreshing parser golden fixtures.

## Contribution workflow

- Keep changes aligned with the v1 contract: release only raw `lymals` binaries/checksums, keep editor integration docs-only, and do not add Lua execution paths to shipped features.
- Run the validation commands above before opening a PR; at minimum, formatting, clippy, tests, docs, and `cargo deny check` should pass locally or in CI.
- Update user/developer docs when behavior, flags, configuration, safety limits, or packaging scope changes.
- Add or update focused tests with code changes; use snapshot/golden updates only when the behavior change is intentional.
- Document externally visible changes in `CHANGELOG.md` under `Unreleased`.

## Architecture notes

- `tower-lsp` owns protocol dispatch in `src/server.rs`; feature-specific request logic lives under `src/handlers/`.
- Feature engines consume local parser/tokenizer/syntax/semantic facades rather than raw upstream AST types.
- Source-line or indentation heuristics are allowed only as supplements after syntax lookup (for example folding/selection/code-action edit extents), and should stay covered by focused tests.
- `src/imports.rs` and `src/workspace.rs` enforce resolver containment and workspace limits.
- `src/eval.rs` is a fail-closed placeholder; do not call it from shipped editor features for v1.
