# Neovim setup

Install a released `lymals` binary and either put it on `PATH` or use an absolute path.

Minimal Neovim built-in LSP setup:

```lua
vim.api.nvim_create_autocmd({ "BufRead", "BufNewFile" }, {
  pattern = "*.lyma",
  callback = function()
    vim.bo.filetype = "lyma"
  end,
})

vim.api.nvim_create_autocmd("FileType", {
  pattern = "lyma",
  callback = function(args)
    vim.lsp.start({
      name = "lymals",
      cmd = { "lymals", "--stdio" },
      root_dir = vim.fs.root(args.buf, { ".git" }) or vim.fn.getcwd(),
      settings = {
        lymals = {
          evaluation = { enabled = false },
        },
      },
    })
  end,
})
```

Replace `"lymals"` with the absolute binary path if it is not on `PATH`.

Notes:

- This is a docs-only integration path for v1; no Neovim plugin package is published.
- Do not publish a Neovim plugin/package for `lymals` until versioning, licensing, and release artifact checksum validation are completed and the binary-only v1 policy is intentionally changed.
- `lymals` currently advertises UTF-16 positions, so Neovim should use the negotiated UTF-16 columns around emoji and other non-ASCII text.
- Additional server settings use the `lymals` section from [`docs/configuration.md`](../docs/configuration.md).
