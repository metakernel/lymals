import * as assert from 'node:assert/strict';
import {
  DEFAULT_SERVER_CONFIG,
  sanitizeServerConfig,
  ServerConfig,
  ServerConfigScopes,
} from '../../src/config';

suite('config', () => {
  test('clamps high-risk workspace settings back to safe defaults in untrusted workspaces', () => {
    const rawConfig: ServerConfig = {
      ...DEFAULT_SERVER_CONFIG,
      diagnostics: { ...DEFAULT_SERVER_CONFIG.diagnostics },
      formatting: { ...DEFAULT_SERVER_CONFIG.formatting },
      imports: { ...DEFAULT_SERVER_CONFIG.imports },
      semanticTokens: { ...DEFAULT_SERVER_CONFIG.semanticTokens },
      completion: { ...DEFAULT_SERVER_CONFIG.completion },
      inlayHints: { ...DEFAULT_SERVER_CONFIG.inlayHints },
      evaluation: { enabled: true },
      allowedRoots: ['file:///workspace'],
      allowedSchemes: ['file', 'untitled'],
      allowAbsoluteFileUris: true,
      excludeGlobs: [...DEFAULT_SERVER_CONFIG.excludeGlobs],
    };
    const scopes: ServerConfigScopes = {
      allowedRoots: 'workspace',
      allowedSchemes: 'workspaceFolder',
      allowAbsoluteFileUris: 'workspace',
      evaluationEnabled: 'workspaceFolder',
    };

    const result = sanitizeServerConfig(rawConfig, scopes, false);

    assert.deepEqual(result.config.allowedRoots, []);
    assert.deepEqual(result.config.allowedSchemes, ['file']);
    assert.equal(result.config.allowAbsoluteFileUris, false);
    assert.equal(result.config.evaluation.enabled, false);
    assert.equal(result.warnings.length, 4);
  });

  test('keeps user-scoped high-risk settings in untrusted workspaces', () => {
    const rawConfig: ServerConfig = {
      ...DEFAULT_SERVER_CONFIG,
      diagnostics: { ...DEFAULT_SERVER_CONFIG.diagnostics },
      formatting: { ...DEFAULT_SERVER_CONFIG.formatting },
      imports: { ...DEFAULT_SERVER_CONFIG.imports },
      semanticTokens: { ...DEFAULT_SERVER_CONFIG.semanticTokens },
      completion: { ...DEFAULT_SERVER_CONFIG.completion },
      inlayHints: { ...DEFAULT_SERVER_CONFIG.inlayHints },
      evaluation: { enabled: true },
      allowedRoots: ['file:///user-approved'],
      allowedSchemes: ['file', 'untitled'],
      allowAbsoluteFileUris: true,
      excludeGlobs: [...DEFAULT_SERVER_CONFIG.excludeGlobs],
    };
    const scopes: ServerConfigScopes = {
      allowedRoots: 'user',
      allowedSchemes: 'user',
      allowAbsoluteFileUris: 'user',
      evaluationEnabled: 'user',
    };

    const result = sanitizeServerConfig(rawConfig, scopes, false);

    assert.deepEqual(result.config, rawConfig);
    assert.deepEqual(result.warnings, []);
  });
});
