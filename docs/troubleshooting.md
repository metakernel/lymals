# Troubleshooting

## Logging

`lumals` writes logs to stderr by default so LSP protocol messages never share stdout. Use `--log-file PATH` to redirect structured logs and panic messages to a file:

```text
lumals --stdio --log-file /tmp/lumals.log
```

Set `RUST_LOG` for tracing filters, for example `RUST_LOG=lumals=debug`.

## Runtime status

Use `workspace/executeCommand` with `lumals.serverStatus` to inspect lifecycle phase, trace setting, configured log level, workspace folder count, open document count, and watched-file invalidation count.

## Diagnostics and imports

Use `lumals.explainDiagnostic` for a diagnostic code explanation. Import/include resolution remains parse-only and root-bounded by default; blocked paths, missing files, cycles, and depth/edge/size limits surface as diagnostics.

## Stale results

Diagnostics are guarded by document version. If a parse result finishes after a newer edit, `lumals` discards the stale result instead of publishing it.

## Editor clients

- VS Code-compatible clients: v1 ships no VSIX. Use a generic/custom LSP client configured for stdio and point it at the released `lumals` binary. See `editors/vscode/README.md`.
- Neovim: use `vim.lsp.start` and ensure `root_dir` is the intended workspace root. See `editors/neovim.md`.
- `lumals` currently advertises UTF-16 positions to every client. If positions look wrong around emoji or non-ASCII text, make sure the client honors the negotiated `positionEncoding: "utf-16"`.
