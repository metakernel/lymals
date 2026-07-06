# Lymals VS Code extension scaffold

This directory contains a local-development VS Code extension scaffold for launching the `lymals` language server over stdio.

## Status

- scaffold only; not published
- VSIX artifacts must remain local-only
- later tasks will add stronger executable resolution and workspace-trust behavior

## Development

```bash
npm install
npm run compile
```

Use the included `.vscode/launch.json` and `.vscode/tasks.json` to run the extension in an Extension Development Host.

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
- `lymalsExtension.server.args`: extra arguments before the extension-appended `--stdio`
- `lymalsExtension.server.allowUntitled`: opt in to untitled Lyma buffers
- `lymalsExtension.server.logFile`: optional `--log-file` path for the server
- `lymalsExtension.server.rustLog`: optional `RUST_LOG` value for the server process
- `lymalsExtension.server.trace.server`: LSP trace level
- `lymalsExtension.server.build.command|args|cwd|buildOnActivation`: optional trusted build-on-activation flow
- `lymalsExtension.trace.client`: extension output verbosity

## License

See the repository license at `../../LICENSE.md`.
