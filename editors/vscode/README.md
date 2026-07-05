# VS Code-compatible clients

`lumals` v1 does not ship a VSIX. Use any VS Code-compatible extension that can launch a custom stdio language server, or a local development extension, and point it at the downloaded `lumals` binary.

Example settings shape:

```json
{
  "lumals.server.path": "/absolute/path/to/lumals",
  "lumals.server.args": ["--stdio"],
  "files.associations": {
    "*.luma": "luma"
  }
}
```

On Windows, use an escaped absolute path such as `"C:\\Tools\\lumals\\lumals.exe"`.

The server writes protocol messages to stdout and logs to stderr or the configured log file only.
