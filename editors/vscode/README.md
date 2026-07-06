# VS Code-compatible clients

`lymals` v1 does not ship a VSIX. Use any VS Code-compatible extension that can launch a custom stdio language server, or a local development extension, and point it at the downloaded `lymals` binary.

## What to configure

- command: absolute path to `lymals`/`lymals.exe`
- args: `--stdio`
- language id / file association: map `*.lyma` to `lyma`
- workspace settings section: `lymals`

Keep logs off stdout; `lymals` reserves stdout for JSON-RPC and writes logs to stderr or `--log-file`.

## Example generic client settings

Client launch settings are extension-specific and are **not** part of the server's `lymals` workspace configuration. Use whatever keys your chosen VS Code-compatible client expects for command/args, for example:

```json
{
  "<client-specific server path setting>": "/absolute/path/to/lymals",
  "<client-specific server args setting>": ["--stdio"],
  "files.associations": {
    "*.lyma": "lyma"
  }
}
```

If you need a concrete example, keep client-only launch settings under an extension namespace such as `lymalsExtension.server.path` / `lymalsExtension.server.args`, not under `lymals`.

On Windows, use an escaped absolute path such as `"C:\\Tools\\lymals\\lymals.exe"`.

If your client supports passing workspace settings to the server, use the normal `lymals` config section documented in `docs/configuration.md` only for actual server settings, for example:

```json
{
  "lymals": {
    "evaluation": { "enabled": false },
    "allowedSchemes": ["file"]
  }
}
```

This setup is intentionally docs-only for v1; no editor marketplace package is published.

Do not publish a VSIX or marketplace release until versioning, licensing, and release artifact checksum validation are completed and the binary-only v1 policy is intentionally changed.
