# Upstream Luma integration research

## Accepted upstream revision

- Repository: `https://github.com/metakernel/luma`
- Accepted git revision: `fab0c21b1e32b837a37a8ef738fd2b64364d6f81`
- Local Cargo pin: `luma = { git = "https://github.com/metakernel/luma", rev = "fab0c21b1e32b837a37a8ef738fd2b64364d6f81" }`

## Licensing

- Repository `LICENSE.md`: MIT (`Copyright (c) 2026 Vincent Roy`).
- Upstream `Cargo.toml` metadata says `MIT OR Apache-2.0`, but the inspected repository snapshot only ships MIT license text.
- Go-forward note: direct git dependency use is MIT-compatible for this repo; vendoring/copying should preserve the upstream MIT notice verbatim, record copied file paths plus source commit, and re-check Apache metadata if upstream clarifies licensing later.
- Current state: `lumals` does **not** vendor/copy upstream `metakernel/luma` source files into `src/`; it consumes upstream through the optional pinned git dependency only.

## Feature flags to use

- Upstream default feature: `parser`.
- Local integration should keep `default-features = false` and enable only `luma/parser` through the local `upstream-luma` feature.
- Rationale: v1 is parse-only; do **not** enable `runtime`, `eval`, `omnilua`, or `engine-omnilua`.
- `parser` pulls `syntax`, which is required for spans, diagnostics, AST, and tokens.

## Stable upstream APIs worth integrating

Prefer the facade crate for dependency stability and narrower imports:

- `luma::parser::parse_str(FileId, name, text) -> Parsed`
- `luma::parser::parse_source(SourceText) -> Parsed`
- `luma::parser::lex_str(FileId, name, text) -> Lexed`
- `luma::parser::lex_source(SourceText) -> Lexed`
- `luma::parser::format_str(FileId, name, text) -> ParsedFormatting`
- `luma::parser::format_parsed(...) -> ParsedFormatting`
- `luma::parser::format_file(...) -> ParsedFormatting`
- `luma::tooling::format_document_text_edit(name, source) -> (ParsedFormatting, TextEdit)`

Stable syntax/data types exposed by `luma::syntax` / `luma_parser` re-exports:

- source identity/ranges: `FileId`, `LumaSource`, `Span`
- diagnostics: `Diagnostic`, `DiagnosticCode`, `Severity`
- tokens: `Token`, `TokenKind`
- parsed AST roots and common nodes: `LumaFile`, `Document`, `DocumentItem`, `LumaNode`

## Observed support matrix

### Available now

- Parse-only AST construction without executing Lua.
- Lexer/token stream APIs (`Lexed`, `Token`, `TokenKind`).
- Source spans via `Span` and source model helpers via `LumaSource`.
- Structured diagnostics with severity and codes.
- Full-document canonical formatting.
- Whole-document text-edit helper for editors.

### Missing / insufficient for direct raw LSP use

- No CST/trivia-preserving tree for whitespace/comment-sensitive editor features.
- No parent links or stable local node IDs for robust incremental cross-references.
- No fine-grained identifier subspan API for all symbol/navigation use-cases.
- No minimal-edit or range-formatting API; formatting is full-document oriented.
- No semantic tokens, references, rename, or workspace-index APIs.
- No direct LSP-oriented incremental parse API surfaced as a stable contract.

## Integration limitations and safety notes

- Parser/formatter APIs are engine-agnostic and safe by default.
- Do not execute Lua for v1; keep evaluation features disabled.
- Imports/includes/modules/tags/schema validation require explicit host wiring upstream and are out of scope for v1 parse-only behavior.
- Directly exposing raw upstream syntax types across the entire server would over-couple `lumals` to upstream AST details and still not cover many LSP features.

## Maintenance strategy

- Prefer the optional pinned git dependency over copying upstream source.
- Keep all upstream usage behind local adapters/facades such as `src/parser/upstream.rs`; local fallback logic in `src/parser/fallback.rs` remains a separate implementation, not a vendored upstream copy.
- When updating upstream, change the Cargo pin and this document together, then re-run `cargo deny check licenses sources` and parser/formatting regression tests.
- If copying upstream ever becomes unavoidable, preserve upstream copyright/license text in-tree and document: repository, commit, copied files, local modifications, owner for rebases, and exit plan back to dependency-based integration.

## Go / no-go decision

- **Go** for using upstream Luma as a backend dependency for parsing, lexing, diagnostics, spans, and whole-document formatting.
- **No-go** for building the language server directly on raw upstream APIs alone.
- Recommended architecture: keep upstream behind a local `lumals` syntax facade and supplemental scanner/indexing layer, so v1 stays parse-only and future backend swaps or upstream API changes remain localized.

## Local conformance coverage

`tests/fixtures/conformance/` recreates representative public Luma syntax categories without copying upstream fixtures. Current status:

- Core directives, lets, mappings, sequences, scalars, block strings, and Lua-expression syntax: **pass** in parse-only mode.
- Multi-document files with schema/include directives: **pass** syntactically; resolver behavior stays local/root-bounded.
- Unsafe import/include policy cases and indentation/duplicate-key validations: **expected diagnostics**.
- Evaluation-dependent Lua runtime behavior: **intentionally unsupported** in v1; syntax parses but no Lua executes.
