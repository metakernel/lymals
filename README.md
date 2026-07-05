# lumals

`lumals` is a parse-only language server for [Luma](https://github.com/metakernel/luma) files ending in `.luma`.

## Status

The server currently supports stdio LSP lifecycle, document sync, diagnostics, configuration, workspace folders/file watching, local import/include/schema resolution, completion, hover, navigation, references, rename, symbols, formatting, semantic tokens, code actions, inlay hints, folding ranges, selection ranges, and safe commands.

Luma evaluation is intentionally **not shipped in v1**. Embedded Lua is parsed/tokenized for editor features but never executed.

## Install / run

Download release archives and checksums from GitHub releases, or build from source:

```text
cargo build --release
target/release/lumals --version
target/release/lumals --stdio
```

See [`docs/packaging.md`](docs/packaging.md) for release artifact names, checksum verification, and local release-equivalent builds.

Useful flags:

```text
lumals --version
lumals --print-config-schema
lumals --stdio --log-file /tmp/lumals.log
```

## VS Code-compatible client setup

Use any generic LSP client extension that can launch a command over stdio. Configure it with:

```json
{
  "languageserver": {
    "luma": {
      "command": "/absolute/path/to/lumals",
      "args": ["--stdio"],
      "filetypes": ["luma"],
      "rootPatterns": [".git"],
      "settings": { "lumals": { "evaluation": { "enabled": false } } }
    }
  }
}
```

Associate files matching `*.luma` with the `luma` language id.

See [`editors/vscode/README.md`](editors/vscode/README.md) for binary-based VS Code-compatible setup notes.

## Neovim setup

```lua
vim.api.nvim_create_autocmd({ "BufRead", "BufNewFile" }, {
  pattern = "*.luma",
  callback = function(args)
    vim.bo[args.buf].filetype = "luma"
  end,
})

vim.lsp.start({
  name = "lumals",
  cmd = { "/absolute/path/to/lumals", "--stdio" },
  root_dir = vim.fs.root(0, { ".git" }) or vim.fn.getcwd(),
  settings = { lumals = { evaluation = { enabled = false } } },
})
```

See [`editors/neovim.md`](editors/neovim.md) for a reusable Neovim setup snippet.

## Configuration

See [`docs/configuration.md`](docs/configuration.md). Defaults are safe: only `file:` schemes are allowed, absolute file URIs are blocked, imports are root-bounded, and evaluation is disabled/reserved.

## Safety model

See [`docs/security.md`](docs/security.md). `lumals` blocks network/package schemes by default, rejects parent traversal, caps resolver depth/edges/file sizes, and keeps all editor features parse-only.

## Feature matrix

See [`docs/feature-matrix.md`](docs/feature-matrix.md) for current support and parse-only semantics.

## Troubleshooting

See [`docs/troubleshooting.md`](docs/troubleshooting.md). The `lumals.serverStatus` command reports lifecycle/config status, and `lumals.explainDiagnostic` explains diagnostic codes.

## Development

```text
cargo fmt --all --check
cargo test
cargo test --all-features
cargo doc --workspace --all-features --no-deps
```

Dependency rationale and workflows are in [`docs/development.md`](docs/development.md). Upstream parser strategy is documented in [`docs/upstream-luma.md`](docs/upstream-luma.md).
