# Feature matrix

| Feature | Status | Parse-only semantics |
| --- | --- | --- |
| Go to definition | Implemented | Navigates to local `let`/alias/key definitions and static import/include/use targets or imported key paths when resolvable without evaluation. |
| Go to declaration | Implemented | Same static target set as definition in v1 parse-only mode. |
| Go to type definition | Implemented | Navigates to static schema/tag/profile/type-like metadata anchors when a matching `@schema`, `@profile`, or fallback tag anchor exists; returns no result when the symbol is not type-like. |
| Go to implementation | Implemented | Navigates to statically imported/included concrete documents or imported key paths when a reference resolves through `@import`/`@include`/`@use`; returns no result for non-resolver constructs such as local scalars, tags, profiles, and metadata-only type names. |
| Import/include/schema resolution | Implemented | Resolves local workspace `file:`/relative targets only, with canonical root containment, cycle detection, depth/edge/file-size guards, and no registry/network access. |
| Evaluation-aware features | Reserved | Disabled by default and not shipped in v1. `evaluation.enabled = true` only reports a reserved extension state; no Lua code is executed. |

## Notes

- v1 remains parse-only: editor features never evaluate Lua or schema/tag resolvers.
- Empty results are intentional when richer answers would require semantic evaluation.
