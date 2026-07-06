# Security model

`lumals` v1 is parse-only. It parses `.luma` files, resolves local workspace imports/includes, and serves editor features without executing embedded Lua.

## Evaluation

- `evaluation.enabled` defaults to `false`.
- Setting `evaluation.enabled` to `true` does **not** enable runtime execution in v1.
- `src/eval.rs` is a reserved fail-closed extension point for a future sandboxed evaluator.
- No shipped/default LSP feature calls an evaluator for diagnostics, completion, hover, navigation (definition/declaration/type/implementation), references, symbols, rename, formatting, semantic tokens, folding, selection ranges, code actions, commands, import resolution, file watching, or indexing.
- When a richer answer would require runtime Lua evaluation, v1 returns partial/empty results instead of executing code.

Future evaluation support requires a separate trust model, sandbox design, and explicit opt-in configuration before any Lua code can run.

## Filesystem and imports

Import/include/schema resolution is local and root-bounded by default. `lumals` blocks parent traversal, non-`file:` schemes, and targets outside configured workspace roots unless explicitly configured otherwise.

Default import/indexing guardrails:

- allowed URI schemes: `file` only;
- no network/package registry resolution;
- no parent traversal (`..`) in import/include targets;
- no absolute file paths unless explicitly enabled;
- target paths must remain under workspace folders or configured `allowedRoots` after normalization/canonicalization, including the canonical target of any symlinked `.luma` path;
- import path completion suggests only local `.luma` files reachable under the current workspace/root-bounded base directory;
- missing files, oversized files, cycles, depth limits, and edge limits report diagnostics instead of panicking;
- if no workspace/configured root exists, resolver policy fails closed instead of allowing arbitrary filesystem reads.

## Input robustness

Malformed files are parsed tolerantly. LSP handlers should return partial/empty results or diagnostics instead of panicking. Stale diagnostics are guarded by document version so older parse results are not published over newer edits.

## Commands

Built-in `workspace/executeCommand` handlers are parse-only/read-only. `lumals.formatWorkspaceFile` is bounded to `file:` `.luma` paths under configured workspace roots, validates that the canonical target stays under those roots, rejects non-regular files/symlink escapes, and returns preview text instead of mutating any file; unsafe or malformed arguments fail closed with JSON-RPC invalid-params errors.

## Dependencies and releases

CI runs formatting, clippy, tests, docs, and `cargo deny check`. The deny policy blocks unknown registries and unknown git sources, allows the pinned optional upstream Luma git source, warns on duplicate transitive crates, and checks advisory/license metadata.

Release guardrails for v1:

- only raw `lumals` archives plus SHA-256 checksum files are in scope;
- no crates.io publish (`publish = false`), VSIX, Neovim plugin, or other editor-package publish occurs automatically;
- release workflows produce artifacts for review/validation first;
- versioning, licensing, and checksum validation must be completed before any later manual publishing/release step is allowed.
