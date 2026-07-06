# lymals commands

All built-in commands are parse-only and return data without executing Lua or mutating files.

## Registered commands

- `lymals.restartIndex`
  - Arguments: none
  - Returns workspace/open-document indexing stats after a safe rebuild.
- `lymals.showSyntaxTree`
  - Arguments: `[{ "uri": "file:///path/to/file.lyma" }]` or `[
    "file:///path/to/file.lyma"
    ]`
  - Returns a textual tree rendered from lymals' local syntax facade; parse-only and read-only.
- `lymals.showConfig`
  - Arguments: none
  - Returns the active `lymals` configuration and workspace folders.
- `lymals.formatWorkspaceFile`
  - Arguments: `[{ "uri": "file:///path/to/file.lyma" }]`
  - Returns formatted text for a workspace-bounded `.lyma` file without writing it or mutating any file.
- `lymals.explainDiagnostic`
  - Arguments: `[{ "code": "L003" }]` or `["L003"]`
  - Returns a short explanation and remediation for a known diagnostic code.
- `lymals.serverStatus`
  - Arguments: none
  - Returns lifecycle, trace, log-level, workspace, open-document, and watcher invalidation status for troubleshooting.

## Safety rules

- Commands fail closed on invalid or missing arguments.
- `formatWorkspaceFile` only reads `file:` URIs that remain inside configured workspace roots.
- `formatWorkspaceFile` is a preview command: it returns formatted text and never writes to disk.
- Errors are returned as safe JSON-RPC invalid-params responses with no secret or stack output.
