# Security model

`lumals` v1 is parse-only. It parses `.luma` files, resolves local workspace imports/includes, and serves editor features without executing embedded Lua.

## Evaluation

- `evaluation.enabled` defaults to `false`.
- Setting `evaluation.enabled` to `true` does **not** enable runtime execution in v1.
- `src/eval.rs` is a reserved fail-closed extension point for a future sandboxed evaluator.
- No shipped LSP feature calls an evaluator for diagnostics, completion, hover, navigation, references, rename, symbols, formatting, semantic tokens, code actions, inlay hints, folding, selection ranges, commands, imports, file watching, or indexing.

Future evaluation support requires a separate trust model, sandbox design, and explicit opt-in configuration before any Lua code can run.

## Filesystem and imports

Import/include/schema resolution is local and root-bounded by default. `lumals` blocks parent traversal, non-`file:` schemes, and targets outside configured workspace roots unless explicitly configured otherwise.

Default import/indexing guardrails:

- allowed URI schemes: `file` only;
- no network/package registry resolution;
- no parent traversal (`..`) in import/include targets;
- no absolute file paths unless explicitly enabled;
- target paths must remain under workspace folders or configured `allowedRoots` after normalization/canonicalization;
- missing files, oversized files, cycles, depth limits, and edge limits report diagnostics instead of panicking;
- if no workspace/configured root exists, resolver policy fails closed instead of allowing arbitrary filesystem reads.

## Input robustness

Malformed files are parsed tolerantly. LSP handlers should return partial/empty results or diagnostics instead of panicking. Stale diagnostics are guarded by document version so older parse results are not published over newer edits.

## Dependencies and releases

CI runs formatting, clippy, tests, docs, and `cargo deny check`. The deny policy blocks unknown registries and unknown git sources, allows the pinned optional upstream Luma git source, warns on duplicate transitive crates, and checks advisory/license metadata. Release automation publishes raw `lumals` archives plus SHA-256 checksum files only; editor-specific packages are not published in v1.
