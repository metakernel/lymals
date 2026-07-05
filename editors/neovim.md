# Neovim setup

Install a released `lumals` binary and either put it on `PATH` or use an absolute path.

Minimal Neovim built-in LSP setup:

```lua
vim.api.nvim_create_autocmd({ "BufRead", "BufNewFile" }, {
  pattern = "*.luma",
  callback = function()
    vim.bo.filetype = "luma"
  end,
})

vim.api.nvim_create_autocmd("FileType", {
  pattern = "luma",
  callback = function(args)
    vim.lsp.start({
      name = "lumals",
      cmd = { "lumals", "--stdio" },
      root_dir = vim.fs.root(args.buf, { ".git" }) or vim.fn.getcwd(),
    })
  end,
})
```

Replace `"lumals"` with the absolute binary path if it is not on `PATH`.
