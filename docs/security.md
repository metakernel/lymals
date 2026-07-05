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
