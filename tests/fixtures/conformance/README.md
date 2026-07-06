# Lyma conformance fixtures

These fixtures recreate public Lyma syntax categories used by `lymals` v1 without copying upstream test data.

- `valid_core.lyma`: expected to parse and format idempotently in parse-only mode.
- `valid_multidoc.lyma`: expected to parse and format idempotently in parse-only mode.
- `invalid_policy.lyma`: expected to parse with validation diagnostics for unsafe/default-blocked constructs.
- `unsupported_eval.lyma`: expected to parse, but evaluation-dependent behavior is intentionally unsupported in v1.

`tests/conformance_tests.rs` records pass/fail/unsupported status for these representative cases.
