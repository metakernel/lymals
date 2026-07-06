# lymals

`lymals` is a parse-only language server for [Lyma](https://github.com/metakernel/lyma) files ending in `.lyma`.

## Status

The server currently supports stdio LSP lifecycle, document sync, diagnostics, configuration, workspace folders/file watching, local import/include/schema resolution, completion, hover, navigation, references, rename, symbols, formatting, semantic tokens, code actions, inlay hints, folding ranges, selection ranges, and safe commands.

Lyma evaluation is intentionally **not shipped in v1**. Embedded Lua is parsed/tokenized for editor features but never executed.

## Install / run

Download release archives and checksums from GitHub releases, or build from source:

```text
cargo build --release
target/release/lymals --version
target/release/lymals --stdio
```

See [`docs/packaging.md`](docs/packaging.md) for release artifact names, checksum verification, and local release-equivalent builds.

Guardrail: v1 is binary-only. Do not publish crates, VSIX/editor packages, or GitHub releases until versioning, licensing, and release checksum validation are complete.

Useful flags:

```text
lymals --version
lymals --print-config-schema
lymals --stdio --log-file /tmp/lymals.log
```

## VS Code-compatible client setup

Use any generic LSP client extension that can launch a command over stdio. Configure it with:

```json
{
  "languageserver": {
    "lyma": {
      "command": "/absolute/path/to/lymals",
      "args": ["--stdio"],
      "filetypes": ["lyma"],
      "rootPatterns": [".git"],
      "settings": { "lymals": { "evaluation": { "enabled": false } } }
    }
  }
}
```

Associate files matching `*.lyma` with the `lyma` language id.

See [`editors/vscode/README.md`](editors/vscode/README.md) for binary-based VS Code-compatible setup notes.

## Neovim setup

```lua
vim.api.nvim_create_autocmd({ "BufRead", "BufNewFile" }, {
  pattern = "*.lyma",
  callback = function(args)
    vim.bo[args.buf].filetype = "lyma"
  end,
})

vim.lsp.start({
  name = "lymals",
  cmd = { "/absolute/path/to/lymals", "--stdio" },
  root_dir = vim.fs.root(0, { ".git" }) or vim.fn.getcwd(),
  settings = { lymals = { evaluation = { enabled = false } } },
})
```

See [`editors/neovim.md`](editors/neovim.md) for a reusable Neovim setup snippet.

## Configuration

See [`docs/configuration.md`](docs/configuration.md). Defaults are safe: only `file:` schemes are allowed, absolute file URIs are blocked, imports are root-bounded, and evaluation is disabled/reserved.

## Safety model

See [`docs/security.md`](docs/security.md). `lymals` blocks network/package schemes by default, rejects parent traversal, caps resolver depth/edges/file sizes, and keeps all editor features parse-only.

## Feature matrix

See [`docs/feature-matrix.md`](docs/feature-matrix.md) for current support and parse-only semantics.

## Troubleshooting

See [`docs/troubleshooting.md`](docs/troubleshooting.md). The `lymals.serverStatus` command reports lifecycle/config status, and `lymals.explainDiagnostic` explains diagnostic codes.

## Development

```text
cargo fmt --all --check
cargo test
cargo test --all-features
cargo doc --workspace --all-features --no-deps
```

For the VS Code extension developer loop:

```text
cargo build
# then in editors/vscode
npm install
npm run compile    # or: npm run watch
```

Open `editors/vscode` in VS Code, set `lymalsExtension.server.path` to `../../target/debug/lymals.exe` on Windows or `../../target/debug/lymals` on macOS/Linux, then press `F5` to launch an Extension Development Host. Open **Output > Lymals** for client/LSP logs; use `lymalsExtension.server.logFile` and `lymalsExtension.server.rustLog` when you need server-side logging.

Dependency rationale and workflows are in [`docs/development.md`](docs/development.md). VS Code-specific setup notes are in [`editors/vscode/README.md`](editors/vscode/README.md). Upstream parser strategy is documented in [`docs/upstream-lyma.md`](docs/upstream-lyma.md).

## Contributing

Contributions should stay within the v1 scope: binary-only releases, docs-only editor setup, and parse-only features that never execute Lua. Release/publishing changes must preserve the guardrail that artifacts are validated for versioning, licensing, and checksums before any publish/release action is introduced. See [`docs/development.md`](docs/development.md) for local workflow, test commands, and PR expectations.
