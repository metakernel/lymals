# lumals commands

All built-in commands are parse-only and return data without executing Lua or mutating files.

## Registered commands

- `lumals.restartIndex`
  - Arguments: none
  - Returns workspace/open-document indexing stats after a safe rebuild.
- `lumals.showSyntaxTree`
  - Arguments: `[{ "uri": "file:///path/to/file.luma" }]` or `[
    "file:///path/to/file.luma"
    ]`
  - Returns a textual tree rendered from lumals' local syntax facade; parse-only and read-only.
- `lumals.showConfig`
  - Arguments: none
  - Returns the active `lumals` configuration and workspace folders.
- `lumals.formatWorkspaceFile`
  - Arguments: `[{ "uri": "file:///path/to/file.luma" }]`
  - Returns formatted text for a workspace-bounded `.luma` file without writing it or mutating any file.
- `lumals.explainDiagnostic`
  - Arguments: `[{ "code": "L003" }]` or `["L003"]`
  - Returns a short explanation and remediation for a known diagnostic code.
- `lumals.serverStatus`
  - Arguments: none
  - Returns lifecycle, trace, log-level, workspace, open-document, and watcher invalidation status for troubleshooting.

## Safety rules

- Commands fail closed on invalid or missing arguments.
- `formatWorkspaceFile` only reads `file:` URIs that remain inside configured workspace roots.
- `formatWorkspaceFile` is a preview command: it returns formatted text and never writes to disk.
- Errors are returned as safe JSON-RPC invalid-params responses with no secret or stack output.
