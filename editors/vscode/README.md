# Lymals VS Code extension scaffold

This directory contains a local-development VS Code extension scaffold for launching the `lymals` language server over stdio.

## Status

- scaffold only; not published
- VSIX artifacts must remain local-only
- later tasks will add stronger executable resolution and workspace-trust behavior
- `npm run package` builds a local `.vsix` for development/review only; it does **not** publish, upload, or release anything

## Development

### Quickstart: launch the extension locally

From the repository root, build the Rust server once:

```bash
cargo build
```

Then in `editors/vscode` install dependencies and compile the extension:

```bash
npm install
npm run compile
```

For iterative work you can leave the TypeScript compiler running instead:

```bash
npm run watch
```

Open `editors/vscode` in VS Code, use the included `.vscode/launch.json` / `.vscode/tasks.json`, and press `F5` to start an Extension Development Host.

Before the first launch, point the extension at your local debug server build:

- Windows: set `lymalsExtension.server.path` to `target/debug/lymals.exe`
- macOS/Linux: set `lymalsExtension.server.path` to `target/debug/lymals`

Because the extension folder is nested under `editors/vscode`, the setting is usually easiest to add as a relative path from that workspace:

- Windows: `../../target/debug/lymals.exe`
- macOS/Linux: `../../target/debug/lymals`

After the Extension Development Host opens:

1. Open `samples/basic.lyma` for a small manual-test fixture that is meant for editor testing, not server conformance.
2. Confirm the file is recognized as Lyma.
3. Check Problems for the deliberate `let broken` diagnostic near the end of the sample.
4. Hover `env`, place the cursor after `@`, `?`, `*`, `=`, `|`, or `${` to probe completion triggers, and run **Format Document** to exercise common requests.
5. Run **Lymals: Show Syntax Tree** (or another Lymals command) to verify command plumbing.
6. Open **View → Output** and select **Lymals** to inspect client logs and optional LSP trace output.

Path-handling notes:

- The extension launches the server with a direct executable path plus argv array; do not add manual shell quotes around paths with spaces.
- Windows example: `C:\Program Files\Lymals\lymals.exe`
- macOS/Linux example: `/opt/my apps/lymals`

### Task 16 validation record (Windows, headless environment)

Because this workspace runs in a CI-like/headless Windows environment, a real interactive VS Code GUI session and manual Extension Development Host inspection were **not** available. Validation for Task 16 therefore split into:

- **Verified by automation**
  - `cargo build` succeeded for the Rust server on Windows.
  - `npm run compile` succeeded for the VS Code extension.
  - `npm test` succeeded (`15 passing, 1 pending`) using `@vscode/test-electron` as the closest available Extension Development Host stand-in.
  - `npm run package` and `npm run package:contents` succeeded.
  - During this validation we found and fixed a real startup bug: the VS Code client was launching `lymals` with duplicate `--stdio` flags when pointed at the repository debug binary.

- **Still not genuinely verifiable without an interactive GUI session**
  - visually confirming language mode, semantic coloring, hover rendering, Problems panel rendering, Output panel contents, and command-palette UX
  - manually changing settings through the Settings UI and watching live editor behavior
  - true click/keyboard-driven restart flows in a visible Extension Development Host window

- **Follow-up risk / limitation**
  - The opt-in real-server extension-host smoke test remains intentionally skipped by default in headless runs. Existing Rust/LSP tests cover diagnostics, hover, completion, formatting, commands, and semantic-token protocol behavior, but a maintainer should still run the checklist above in a real VS Code window before calling GUI behavior fully validated.

If you prefer task-driven packaging for a local-only artifact, the repo root currently includes a `Taskfile.yml` `vsix` task. Keep that output local: v1 docs and policy explicitly do **not** treat `.vsix` files as publishable artifacts.

### Local packaging workflow (`.vsix` build only)

From `editors/vscode`:

```bash
npm run package
```

That runs `vsce package --no-dependencies` and produces a local `.vsix` in `editors/vscode/`.

Before treating any `.vsix` as even internally shareable, inspect the package contents:

```bash
npm run package:contents
```

Or run `vsce ls --tree --no-dependencies` directly. Review the listing to confirm no secrets, tests, `node_modules`, Rust `target` output, logs, or other unintended files leaked into the archive.

The manual sample under `samples/basic.lyma` is intentionally excluded from the packaged VSIX via `.vscodeignore`. It exists only to make Extension Development Host testing easy without shipping editor-demo content in the local artifact.

Prepackage notes:

- Default recommendation for this repo: **do not** bundle platform binaries in the first PR.
- For local development, use `lymalsExtension.server.path`, `LYMALS_SERVER_PATH`, or a `lymals` binary already on `PATH`.
- If a future release-binary strategy is chosen, build the server first with `cargo build --release` and then make the binary copy/embed step explicit and reviewable.
- Keep the VS Code extension version aligned with the Rust crate version in `../../Cargo.toml` so local artifacts, release candidates, and later manual publishing decisions refer to the same build identity.

Marketplace/Open VSX prerequisites remain intentionally unmet here: publisher ownership, final versioning policy, licensing/review of shipped files, and release-artifact validation still need separate maintainer approval.

Recommended release artifacts for v1 remain the same as `../../docs/packaging.md`: raw `lymals` platform archives plus matching checksum files, not editor marketplace uploads.

This plan stops at local packaging. It does **not** publish or upload the `.vsix` to the VS Code Marketplace, Open VSX, or GitHub Releases, and no automation should do so until versioning, licensing, and release-artifact validation are separately completed and approved by a maintainer.

### Debug logging

- `lymalsExtension.server.logFile`: passes `--log-file <path>` to the server.
- `lymalsExtension.server.rustLog`: sets `RUST_LOG` for the server process.
- `lymalsExtension.server.trace.server`: writes LSP traffic to **Output > Lymals**.

Examples:

- Windows `lymalsExtension.server.logFile`: `C:\Users\you\AppData\Local\Lymals\logs\server.log`
- macOS/Linux `lymalsExtension.server.logFile`: `/tmp/lymals/server.log`

When the extension runs in development mode and `lymalsExtension.server.logFile` is left empty, it automatically chooses a storage-backed log file so server logging does not compete with stdio. The resolved log path is written to **Output > Lymals** on startup.

## Commands

The extension contributes these Lymals command-palette actions:

- `Lymals: Restart Language Server`
- `Lymals: Show Output`
- `Lymals: Restart Index` — calls server command `lymals.restartIndex` and writes the response to Output > Lymals
- `Lymals: Show Syntax Tree` — calls `lymals.showSyntaxTree` for the active Lyma editor and opens a read-only preview
- `Lymals: Show Config` — calls `lymals.showConfig` and opens the returned JSON/text in a read-only preview
- `Lymals: Format Workspace File Preview` — calls `lymals.formatWorkspaceFile` for the active workspace-backed `.lyma` file and opens the formatted preview without writing to disk
- `Lymals: Explain Diagnostic` — calls `lymals.explainDiagnostic` using the diagnostic under the cursor when possible, or a manually entered code otherwise

Commands that require a URI use the active Lyma editor and fail safely with a VS Code message if no suitable `.lyma` document is active.

## Settings

### Server settings (`lymals.*`)

The extension contributes the full documented `lymals` server configuration surface with defaults from `../../docs/configuration.md`:

- feature toggles: `diagnostics.enabled`, `formatting.enabled`, `imports.enabled`, `semanticTokens.enabled`, `completion.enabled`
- inlay hints: `inlayHints.enabled`, `inlayHints.inferredTypes`, `inlayHints.keyPaths`, `inlayHints.letBindings`, `inlayHints.profileEffects`, `inlayHints.importResolution`
- evaluation: `evaluation.enabled`
- runtime/indexing: `logLevel`, `parserBackend`, `indexWorkspace`, `followImportsInIndex`
- import guardrails: `allowedRoots`, `allowedSchemes`, `allowAbsoluteFileUris`, `excludeGlobs`
- limits: `maxResolveDepth`, `maxResolvedEdgesPerFile`, `maxIndexedFilesPerWorkspace`, `maxIndexedFileBytes`

High-risk workspace-scoped settings (`allowedRoots`, `allowedSchemes`, `allowAbsoluteFileUris`, `evaluation.enabled`) are marked restricted and are clamped back to documented safe defaults in untrusted workspaces before the extension forwards them to the server. The output channel explains when this happens.

### Extension-only settings (`lymalsExtension.*`)

- `lymalsExtension.server.path`: explicit server executable path; defaults to `lymals` from `PATH`
- `lymalsExtension.server.args`: extra arguments passed to `lymals` before VS Code attaches stdio transport
- `lymalsExtension.server.allowUntitled`: opt in to untitled Lyma buffers
- `lymalsExtension.server.logFile`: optional `--log-file` path for the server
- `lymalsExtension.server.rustLog`: optional `RUST_LOG` value for the server process
- `lymalsExtension.server.trace.server`: LSP trace level
- `lymalsExtension.server.build.command|args|cwd|buildOnActivation`: optional trusted build-on-activation flow
- `lymalsExtension.trace.client`: extension output verbosity

## License

See the repository license at `../../LICENSE.md`.
