# Feature matrix

| Feature | Status | Parse-only semantics |
| --- | --- | --- |
| Lifecycle / stdio | Implemented | Initializes, syncs, shuts down, and logs without writing protocol-unsafe stdout. |
| Diagnostics | Implemented | Parser, validation, and resolver diagnostics only; no evaluation. |
| Completion | Implemented | Directives, static values, aliases, workspace keys, and safe local paths. |
| Hover | Implemented | Static syntax/help/type/alias information only. |
| Go to definition | Implemented | Navigates to local `let`/alias/key definitions and static import/include/use targets or imported key paths when resolvable without evaluation. |
| Go to declaration | Implemented | Same static target set as definition in v1 parse-only mode. |
| Go to type definition | Implemented | Navigates to static schema/tag/profile/type-like metadata anchors when a matching `@schema`, `@profile`, or fallback tag anchor exists; returns no result when the symbol is not type-like. |
| Go to implementation | Implemented | Navigates to statically imported/included concrete documents or imported key paths when a reference resolves through `@import`/`@include`/`@use`; returns no result for non-resolver constructs such as local scalars, tags, profiles, and metadata-only type names. |
| References | Implemented | Static references across open/indexed workspace files. |
| Rename | Implemented | Static keys, lets, and aliases; rejects opaque Lua ranges. |
| Document/workspace symbols | Implemented | Static AST/semantic symbols. |
| Formatting/range formatting | Implemented | Conservative parse-preserving formatting; preserves Lua/block contents. |
| Semantic tokens | Implemented | Token-based static highlighting, including Lua syntax regions as tokens only. |
| Code actions | Implemented | Safe text edits such as tabs-to-spaces, quote/null fixes, directive/import organization. |
| Inlay hints | Implemented | Optional low-noise hints; categories are off by default. |
| Folding ranges | Implemented | Structural/comment/block folds without evaluation. |
| Selection ranges | Implemented | Source/token/indentation-derived parent chains. |
| Commands | Implemented | Safe parse-only commands; no filesystem mutation outside roots. |
| Import/include/schema resolution | Implemented | Resolves local workspace `file:`/relative targets only, with canonical root containment, cycle detection, depth/edge/file-size guards, and no registry/network access. |
| Evaluation-aware features | Reserved | Disabled by default and not shipped in v1. `evaluation.enabled = true` only reports a reserved extension state; no Lua code is executed. |

## Notes

- v1 remains parse-only: editor features never evaluate Lua or schema/tag resolvers.
- Empty results are intentional when richer answers would require semantic evaluation.

## Client validation

| Client shape | Status | Caveats |
| --- | --- | --- |
| VS Code-compatible stdio client | Supported via downloaded binary | v1 does not publish a VSIX. Configure a generic/custom LSP client to run `lumals --stdio`, associate `*.luma` with `luma`, and keep logs off stdout. Defaults to UTF-16 positions when the client does not advertise `general.positionEncodings`. |
| Neovim built-in LSP | Supported via downloaded binary | Use `vim.lsp.start` with `cmd = { "lumals", "--stdio" }`; root detection should point at the project/workspace root. UTF-8 positions are used when advertised by the client. |

Automated compatibility coverage lives in `tests/client_compat_tests.rs` and checks representative initialization/capability negotiation plus a basic open/diagnostics/hover round trip.
