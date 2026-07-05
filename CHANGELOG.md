# Changelog

All notable changes to `lumals` are documented here.

## Unreleased

- Initial parse-only Luma language server implementation.
- Stdio transport by default, with `--stdio` as an explicit synonym.
- Diagnostics, completion, hover, navigation, references, rename, symbols, formatting, semantic tokens, code actions, inlay hints, folding ranges, selection ranges, safe commands, workspace indexing, and guarded import/include/schema resolution.
- Parse-only security posture: no Lua execution by default; evaluation remains a reserved fail-closed extension point.
- Release automation for raw `lumals` binaries and SHA-256 checksums.
