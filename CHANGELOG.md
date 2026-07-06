# Changelog

All notable changes to `lymals` are documented here.

## Unreleased

- Initial parse-only Lyma language server implementation.
- Stdio transport by default, with `--stdio` as an explicit synonym.
- Diagnostics, completion, hover, navigation, references, rename, symbols, formatting, semantic tokens, code actions, inlay hints, folding ranges, selection ranges, safe commands, workspace indexing, and guarded import/include/schema resolution.
- Parse-only security posture: no Lua execution by default; evaluation remains a reserved fail-closed extension point.
- Release automation for raw `lymals` binaries and SHA-256 checksums.
- Release guardrail: workflow builds archives/checksums only; no crate/editor package publishing or automatic GitHub release publishing until versioning, licensing, and artifact validation are approved.
- User/developer docs covering binary installation, docs-only VS Code-compatible and Neovim setup, configuration, safety limits, packaging, troubleshooting, upstream parser strategy, and contribution workflow.
