# Configuration

`lumals` reads settings from the `lumals` workspace configuration section.

## Defaults

- `diagnostics.enabled`: `true`
- `formatting.enabled`: `true`
- `imports.enabled`: `true`
- `semanticTokens.enabled`: `true`
- `completion.enabled`: `true`
- `inlayHints.enabled`: `true`
- `evaluation.enabled`: `false` (v1 remains parse-only)
- `logLevel`: `"info"`
- `parserBackend`: `"auto"`
- `indexWorkspace`: `true`
- `followImportsInIndex`: `true`
- `allowedRoots`: `[]`
- `allowedSchemes`: `["file"]`
- `allowAbsoluteFileUris`: `false`
- `excludeGlobs`: `[]`
- `maxResolveDepth`: `16`
- `maxResolvedEdgesPerFile`: `256`
- `maxIndexedFilesPerWorkspace`: `10000`
- `maxIndexedFileBytes`: `1048576`

## Example

```json
{
  "lumals": {
    "diagnostics": { "enabled": true },
    "formatting": { "enabled": true },
    "imports": { "enabled": true },
    "semanticTokens": { "enabled": true },
    "completion": { "enabled": true },
    "inlayHints": { "enabled": false },
    "evaluation": { "enabled": false },
    "logLevel": "debug",
    "parserBackend": "fallback",
    "indexWorkspace": true,
    "followImportsInIndex": true,
    "allowedRoots": ["file:///workspace"],
    "allowedSchemes": ["file"],
    "allowAbsoluteFileUris": false,
    "excludeGlobs": ["**/vendor/**", "**/.git/**"],
    "maxResolveDepth": 8,
    "maxResolvedEdgesPerFile": 128,
    "maxIndexedFilesPerWorkspace": 5000,
    "maxIndexedFileBytes": 524288
  }
}
```

## Protocol behavior

- If the client supports `workspace/configuration`, `lumals` requests the `lumals` section after `initialized`.
- If the client does not support it, or returns invalid data, `lumals` falls back to the defaults above.
- `workspace/didChangeConfiguration` updates the active settings; invalid updates also fall back to defaults.

## Schema

Print the generated JSON schema with:

```text
lumals --print-config-schema
```
