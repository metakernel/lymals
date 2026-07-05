# Luma conformance fixtures

These fixtures recreate public Luma syntax categories used by `lumals` v1 without copying upstream test data.

- `valid_core.luma`: expected to parse and format idempotently in parse-only mode.
- `valid_multidoc.luma`: expected to parse and format idempotently in parse-only mode.
- `invalid_policy.luma`: expected to parse with validation diagnostics for unsafe/default-blocked constructs.
- `unsupported_eval.luma`: expected to parse, but evaluation-dependent behavior is intentionally unsupported in v1.

`tests/conformance_tests.rs` records pass/fail/unsupported status for these representative cases.
