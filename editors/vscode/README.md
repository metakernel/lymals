# VS Code-compatible clients

`lumals` v1 does not ship a VSIX. Use any VS Code-compatible extension that can launch a custom stdio language server, or a local development extension, and point it at the downloaded `lumals` binary.

## What to configure

- command: absolute path to `lumals`/`lumals.exe`
- args: `--stdio`
- language id / file association: map `*.luma` to `luma`
- workspace settings section: `lumals`

Keep logs off stdout; `lumals` reserves stdout for JSON-RPC and writes logs to stderr or `--log-file`.

## Example generic client settings

Client launch settings are extension-specific and are **not** part of the server's `lumals` workspace configuration. Use whatever keys your chosen VS Code-compatible client expects for command/args, for example:

```json
{
  "<client-specific server path setting>": "/absolute/path/to/lumals",
  "<client-specific server args setting>": ["--stdio"],
  "files.associations": {
    "*.luma": "luma"
  }
}
```

If you need a concrete example, keep client-only launch settings under an extension namespace such as `lumalsExtension.server.path` / `lumalsExtension.server.args`, not under `lumals`.

On Windows, use an escaped absolute path such as `"C:\\Tools\\lumals\\lumals.exe"`.

If your client supports passing workspace settings to the server, use the normal `lumals` config section documented in `docs/configuration.md` only for actual server settings, for example:

```json
{
  "lumals": {
    "evaluation": { "enabled": false },
    "allowedSchemes": ["file"]
  }
}
```

This setup is intentionally docs-only for v1; no editor marketplace package is published.

Do not publish a VSIX or marketplace release until versioning, licensing, and release artifact checksum validation are completed and the binary-only v1 policy is intentionally changed.
