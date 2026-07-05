# Packaging and releases

v1 releases publish only standalone `lumals` binary archives plus SHA-256 checksum files.

## Artifacts

- `lumals-linux-x86_64.tar.gz`
- `lumals-macos-aarch64.tar.gz`
- `lumals-windows-x86_64.zip`
- Matching `*.sha256` files

No VS Code, Zed, Neovim, or other editor-specific packages are published for v1.

## Install from a release

1. Download the archive for your platform.
2. Verify the checksum:
   - Unix: `shasum -a 256 <archive>`
   - Windows PowerShell: `Get-FileHash -Algorithm SHA256 <archive>`
3. Extract the archive.
4. Put `lumals`/`lumals.exe` on your `PATH`, or point your editor configuration at the extracted absolute path.
5. Confirm the binary works: `lumals --version`.

## Local release-equivalent build

```sh
cargo build --release --bin lumals
```

The resulting binary is under `target/release/`.

## Release workflow

`.github/workflows/release.yml` runs on tags matching `v*` and on manual dispatch. It builds the release binary on Linux, macOS, and Windows, creates archives, writes SHA-256 checksum files, uploads workflow artifacts, and attaches assets to tagged GitHub releases.
