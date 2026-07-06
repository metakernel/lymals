import * as vscode from 'vscode';

export const SERVER_SECTION = 'lymals';
export const EXTENSION_SECTION = 'lymalsExtension';
const SERVER_PATH_SETTING = 'server.path';
const SERVER_ARGS_SETTING = 'server.args';
const SERVER_ALLOW_UNTITLED_SETTING = 'server.allowUntitled';
const SERVER_LOG_FILE_SETTING = 'server.logFile';
const SERVER_RUST_LOG_SETTING = 'server.rustLog';
const SERVER_TRACE_SETTING = 'server.trace.server';
const SERVER_BUILD_COMMAND_SETTING = 'server.build.command';
const SERVER_BUILD_ARGS_SETTING = 'server.build.args';
const SERVER_BUILD_CWD_SETTING = 'server.build.cwd';
const SERVER_BUILD_ON_ACTIVATION_SETTING = 'server.buildOnActivation';
const CLIENT_TRACE_SETTING = 'trace.client';

const DIAGNOSTICS_ENABLED_SETTING = 'diagnostics.enabled';
const FORMATTING_ENABLED_SETTING = 'formatting.enabled';
const IMPORTS_ENABLED_SETTING = 'imports.enabled';
const SEMANTIC_TOKENS_ENABLED_SETTING = 'semanticTokens.enabled';
const COMPLETION_ENABLED_SETTING = 'completion.enabled';
const INLAY_HINTS_ENABLED_SETTING = 'inlayHints.enabled';
const INLAY_HINTS_INFERRED_TYPES_SETTING = 'inlayHints.inferredTypes';
const INLAY_HINTS_KEY_PATHS_SETTING = 'inlayHints.keyPaths';
const INLAY_HINTS_LET_BINDINGS_SETTING = 'inlayHints.letBindings';
const INLAY_HINTS_PROFILE_EFFECTS_SETTING = 'inlayHints.profileEffects';
const INLAY_HINTS_IMPORT_RESOLUTION_SETTING = 'inlayHints.importResolution';
const EVALUATION_ENABLED_SETTING = 'evaluation.enabled';
const SERVER_LOG_LEVEL_SETTING = 'logLevel';
const PARSER_BACKEND_SETTING = 'parserBackend';
const INDEX_WORKSPACE_SETTING = 'indexWorkspace';
const FOLLOW_IMPORTS_IN_INDEX_SETTING = 'followImportsInIndex';
const ALLOWED_ROOTS_SETTING = 'allowedRoots';
const ALLOWED_SCHEMES_SETTING = 'allowedSchemes';
const ALLOW_ABSOLUTE_FILE_URIS_SETTING = 'allowAbsoluteFileUris';
const EXCLUDE_GLOBS_SETTING = 'excludeGlobs';
const MAX_RESOLVE_DEPTH_SETTING = 'maxResolveDepth';
const MAX_RESOLVED_EDGES_PER_FILE_SETTING = 'maxResolvedEdgesPerFile';
const MAX_INDEXED_FILES_PER_WORKSPACE_SETTING = 'maxIndexedFilesPerWorkspace';
const MAX_INDEXED_FILE_BYTES_SETTING = 'maxIndexedFileBytes';

export type TraceLevel = 'off' | 'messages' | 'verbose';
export type LogLevel = 'debug' | 'info' | 'warn' | 'error';
export type ServerLogLevel = 'error' | 'warn' | 'info' | 'debug' | 'trace';
export type ParserBackend = 'auto' | 'fallback' | 'upstream';
export type ServerPathScope = 'none' | 'user' | 'workspace' | 'workspaceFolder';
export type ConfigurationScope = 'default' | 'user' | 'workspace' | 'workspaceFolder';

export interface FeatureToggleConfig {
  enabled: boolean;
}

export interface InlayHintsConfig extends FeatureToggleConfig {
  inferredTypes: boolean;
  keyPaths: boolean;
  letBindings: boolean;
  profileEffects: boolean;
  importResolution: boolean;
}

export interface ServerConfig {
  diagnostics: FeatureToggleConfig;
  formatting: FeatureToggleConfig;
  imports: FeatureToggleConfig;
  semanticTokens: FeatureToggleConfig;
  completion: FeatureToggleConfig;
  inlayHints: InlayHintsConfig;
  evaluation: FeatureToggleConfig;
  logLevel: ServerLogLevel;
  parserBackend: ParserBackend;
  indexWorkspace: boolean;
  followImportsInIndex: boolean;
  allowedRoots: string[];
  allowedSchemes: string[];
  allowAbsoluteFileUris: boolean;
  excludeGlobs: string[];
  maxResolveDepth: number;
  maxResolvedEdgesPerFile: number;
  maxIndexedFilesPerWorkspace: number;
  maxIndexedFileBytes: number;
}

export interface ServerConfigScopes {
  allowedRoots: ConfigurationScope;
  allowedSchemes: ConfigurationScope;
  allowAbsoluteFileUris: ConfigurationScope;
  evaluationEnabled: ConfigurationScope;
}

export interface SanitizedServerConfig {
  config: ServerConfig;
  warnings: string[];
}

export const DEFAULT_SERVER_CONFIG: ServerConfig = {
  diagnostics: { enabled: true },
  formatting: { enabled: true },
  imports: { enabled: true },
  semanticTokens: { enabled: true },
  completion: { enabled: true },
  inlayHints: {
    enabled: true,
    inferredTypes: false,
    keyPaths: false,
    letBindings: false,
    profileEffects: false,
    importResolution: false,
  },
  evaluation: { enabled: false },
  logLevel: 'info',
  parserBackend: 'auto',
  indexWorkspace: true,
  followImportsInIndex: true,
  allowedRoots: [],
  allowedSchemes: ['file'],
  allowAbsoluteFileUris: false,
  excludeGlobs: [],
  maxResolveDepth: 16,
  maxResolvedEdgesPerFile: 256,
  maxIndexedFilesPerWorkspace: 10000,
  maxIndexedFileBytes: 1048576,
};

export interface BuildTaskConfig {
  command?: string;
  args: string[];
  cwd?: string;
}

export interface ExtensionConfig {
  serverPath?: string;
  serverArgs: string[];
  allowUntitled: boolean;
  serverLogFile?: string;
  rustLog?: string;
  serverPathScope: ServerPathScope;
  build: BuildTaskConfig;
  buildOnActivation: boolean;
  serverTrace: TraceLevel;
  logLevel: LogLevel;
}

export function getExtensionConfig(workspaceFolder?: vscode.WorkspaceFolder): ExtensionConfig {
  const config = vscode.workspace.getConfiguration(EXTENSION_SECTION, workspaceFolder);
  const serverPath = normalizeOptionalString(config.get<string>(SERVER_PATH_SETTING, ''));
  const serverArgs = config.get<string[]>(SERVER_ARGS_SETTING, []).filter((value) => value.length > 0);
  const allowUntitled = config.get<boolean>(SERVER_ALLOW_UNTITLED_SETTING, false);
  const serverLogFile = normalizeOptionalString(config.get<string>(SERVER_LOG_FILE_SETTING, ''));
  const rustLog = normalizeOptionalString(config.get<string>(SERVER_RUST_LOG_SETTING, ''));
  const buildCommand = normalizeOptionalString(config.get<string>(SERVER_BUILD_COMMAND_SETTING, ''));
  const buildArgs = config.get<string[]>(SERVER_BUILD_ARGS_SETTING, []).filter((value) => value.length > 0);
  const buildCwd = normalizeOptionalString(config.get<string>(SERVER_BUILD_CWD_SETTING, ''));
  const buildOnActivation = config.get<boolean>(SERVER_BUILD_ON_ACTIVATION_SETTING, false);
  const serverTrace = config.get<TraceLevel>(SERVER_TRACE_SETTING, 'off');
  const logLevel = config.get<LogLevel>(CLIENT_TRACE_SETTING, 'info');

  return {
    serverPath,
    serverArgs,
    allowUntitled,
    serverLogFile,
    rustLog,
    serverPathScope: getServerPathScope(config, serverPath),
    build: {
      command: buildCommand,
      args: buildArgs,
      cwd: buildCwd,
    },
    buildOnActivation,
    serverTrace,
    logLevel,
  };
}

export function getServerConfig(
  workspaceFolder?: vscode.WorkspaceFolder,
  options: { isTrustedWorkspace?: boolean } = {},
): SanitizedServerConfig {
  const config = vscode.workspace.getConfiguration(SERVER_SECTION, workspaceFolder);

  const rawConfig: ServerConfig = {
    diagnostics: { enabled: readBoolean(config, DIAGNOSTICS_ENABLED_SETTING, DEFAULT_SERVER_CONFIG.diagnostics.enabled) },
    formatting: { enabled: readBoolean(config, FORMATTING_ENABLED_SETTING, DEFAULT_SERVER_CONFIG.formatting.enabled) },
    imports: { enabled: readBoolean(config, IMPORTS_ENABLED_SETTING, DEFAULT_SERVER_CONFIG.imports.enabled) },
    semanticTokens: {
      enabled: readBoolean(config, SEMANTIC_TOKENS_ENABLED_SETTING, DEFAULT_SERVER_CONFIG.semanticTokens.enabled),
    },
    completion: { enabled: readBoolean(config, COMPLETION_ENABLED_SETTING, DEFAULT_SERVER_CONFIG.completion.enabled) },
    inlayHints: {
      enabled: readBoolean(config, INLAY_HINTS_ENABLED_SETTING, DEFAULT_SERVER_CONFIG.inlayHints.enabled),
      inferredTypes: readBoolean(
        config,
        INLAY_HINTS_INFERRED_TYPES_SETTING,
        DEFAULT_SERVER_CONFIG.inlayHints.inferredTypes,
      ),
      keyPaths: readBoolean(config, INLAY_HINTS_KEY_PATHS_SETTING, DEFAULT_SERVER_CONFIG.inlayHints.keyPaths),
      letBindings: readBoolean(
        config,
        INLAY_HINTS_LET_BINDINGS_SETTING,
        DEFAULT_SERVER_CONFIG.inlayHints.letBindings,
      ),
      profileEffects: readBoolean(
        config,
        INLAY_HINTS_PROFILE_EFFECTS_SETTING,
        DEFAULT_SERVER_CONFIG.inlayHints.profileEffects,
      ),
      importResolution: readBoolean(
        config,
        INLAY_HINTS_IMPORT_RESOLUTION_SETTING,
        DEFAULT_SERVER_CONFIG.inlayHints.importResolution,
      ),
    },
    evaluation: { enabled: readBoolean(config, EVALUATION_ENABLED_SETTING, DEFAULT_SERVER_CONFIG.evaluation.enabled) },
    logLevel: readEnum(config, SERVER_LOG_LEVEL_SETTING, DEFAULT_SERVER_CONFIG.logLevel, ['error', 'warn', 'info', 'debug', 'trace']),
    parserBackend: readEnum(config, PARSER_BACKEND_SETTING, DEFAULT_SERVER_CONFIG.parserBackend, ['auto', 'fallback', 'upstream']),
    indexWorkspace: readBoolean(config, INDEX_WORKSPACE_SETTING, DEFAULT_SERVER_CONFIG.indexWorkspace),
    followImportsInIndex: readBoolean(
      config,
      FOLLOW_IMPORTS_IN_INDEX_SETTING,
      DEFAULT_SERVER_CONFIG.followImportsInIndex,
    ),
    allowedRoots: readStringArray(config, ALLOWED_ROOTS_SETTING, DEFAULT_SERVER_CONFIG.allowedRoots),
    allowedSchemes: readStringArray(config, ALLOWED_SCHEMES_SETTING, DEFAULT_SERVER_CONFIG.allowedSchemes),
    allowAbsoluteFileUris: readBoolean(
      config,
      ALLOW_ABSOLUTE_FILE_URIS_SETTING,
      DEFAULT_SERVER_CONFIG.allowAbsoluteFileUris,
    ),
    excludeGlobs: readStringArray(config, EXCLUDE_GLOBS_SETTING, DEFAULT_SERVER_CONFIG.excludeGlobs),
    maxResolveDepth: readNumber(config, MAX_RESOLVE_DEPTH_SETTING, DEFAULT_SERVER_CONFIG.maxResolveDepth),
    maxResolvedEdgesPerFile: readNumber(
      config,
      MAX_RESOLVED_EDGES_PER_FILE_SETTING,
      DEFAULT_SERVER_CONFIG.maxResolvedEdgesPerFile,
    ),
    maxIndexedFilesPerWorkspace: readNumber(
      config,
      MAX_INDEXED_FILES_PER_WORKSPACE_SETTING,
      DEFAULT_SERVER_CONFIG.maxIndexedFilesPerWorkspace,
    ),
    maxIndexedFileBytes: readNumber(
      config,
      MAX_INDEXED_FILE_BYTES_SETTING,
      DEFAULT_SERVER_CONFIG.maxIndexedFileBytes,
    ),
  };

  return sanitizeServerConfig(rawConfig, {
    allowedRoots: getConfigurationScope(config, ALLOWED_ROOTS_SETTING, rawConfig.allowedRoots),
    allowedSchemes: getConfigurationScope(config, ALLOWED_SCHEMES_SETTING, rawConfig.allowedSchemes),
    allowAbsoluteFileUris: getConfigurationScope(
      config,
      ALLOW_ABSOLUTE_FILE_URIS_SETTING,
      rawConfig.allowAbsoluteFileUris,
    ),
    evaluationEnabled: getConfigurationScope(config, EVALUATION_ENABLED_SETTING, rawConfig.evaluation.enabled),
  }, options.isTrustedWorkspace ?? vscode.workspace.isTrusted);
}

export function sanitizeServerConfig(
  config: ServerConfig,
  scopes: ServerConfigScopes,
  isTrustedWorkspace: boolean,
): SanitizedServerConfig {
  const sanitizedConfig: ServerConfig = cloneServerConfig(config);
  const warnings: string[] = [];

  if (!isTrustedWorkspace) {
    if (isWorkspaceScoped(scopes.allowedRoots) && config.allowedRoots.length > 0) {
      sanitizedConfig.allowedRoots = [...DEFAULT_SERVER_CONFIG.allowedRoots];
      warnings.push(
        'Ignoring workspace-scoped lymals.allowedRoots in an untrusted workspace because it broadens import resolution beyond the documented safe default [].',
      );
    }

    if (isWorkspaceScoped(scopes.allowedSchemes) && broadensAllowedSchemes(config.allowedSchemes)) {
      sanitizedConfig.allowedSchemes = [...DEFAULT_SERVER_CONFIG.allowedSchemes];
      warnings.push(
        'Ignoring workspace-scoped lymals.allowedSchemes in an untrusted workspace because it broadens import resolution beyond the documented safe default ["file"].',
      );
    }

    if (isWorkspaceScoped(scopes.allowAbsoluteFileUris) && config.allowAbsoluteFileUris) {
      sanitizedConfig.allowAbsoluteFileUris = DEFAULT_SERVER_CONFIG.allowAbsoluteFileUris;
      warnings.push(
        'Ignoring workspace-scoped lymals.allowAbsoluteFileUris in an untrusted workspace because the documented safe default is false.',
      );
    }

    if (isWorkspaceScoped(scopes.evaluationEnabled) && config.evaluation.enabled) {
      sanitizedConfig.evaluation.enabled = DEFAULT_SERVER_CONFIG.evaluation.enabled;
      warnings.push(
        'Ignoring workspace-scoped lymals.evaluation.enabled in an untrusted workspace because the documented safe default is false.',
      );
    }
  }

  return {
    config: sanitizedConfig,
    warnings,
  };
}

function getServerPathScope(
  config: vscode.WorkspaceConfiguration,
  effectiveValue: string | undefined,
): ServerPathScope {
  if (!effectiveValue) {
    return 'none';
  }

  const inspected = config.inspect<string>(SERVER_PATH_SETTING);
  if (!inspected) {
    return 'user';
  }

  if (normalizeOptionalString(inspected.workspaceFolderValue ?? '') === effectiveValue) {
    return 'workspaceFolder';
  }

  if (normalizeOptionalString(inspected.workspaceValue ?? '') === effectiveValue) {
    return 'workspace';
  }

  return 'user';
}

function normalizeOptionalString(value: string): string | undefined {
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : undefined;
}

function readBoolean(config: vscode.WorkspaceConfiguration, key: string, fallback: boolean): boolean {
  const value = config.get<unknown>(key, fallback);
  return typeof value === 'boolean' ? value : fallback;
}

function readNumber(config: vscode.WorkspaceConfiguration, key: string, fallback: number): number {
  const value = config.get<unknown>(key, fallback);
  return typeof value === 'number' && Number.isFinite(value) ? value : fallback;
}

function readEnum<T extends string>(
  config: vscode.WorkspaceConfiguration,
  key: string,
  fallback: T,
  allowedValues: readonly T[],
): T {
  const value = config.get<unknown>(key, fallback);
  return typeof value === 'string' && allowedValues.includes(value as T) ? (value as T) : fallback;
}

function readStringArray(config: vscode.WorkspaceConfiguration, key: string, fallback: readonly string[]): string[] {
  const value = config.get<unknown>(key, fallback);
  if (!Array.isArray(value)) {
    return [...fallback];
  }

  return value
    .filter((item): item is string => typeof item === 'string')
    .map((item) => item.trim())
    .filter((item) => item.length > 0);
}

function getConfigurationScope<T>(
  config: vscode.WorkspaceConfiguration,
  key: string,
  effectiveValue: T,
): ConfigurationScope {
  const inspected = config.inspect<T>(key);
  if (!inspected) {
    return 'default';
  }

  if (valuesEqual(inspected.workspaceFolderValue, effectiveValue)) {
    return 'workspaceFolder';
  }

  if (valuesEqual(inspected.workspaceValue, effectiveValue)) {
    return 'workspace';
  }

  if (valuesEqual(inspected.globalValue, effectiveValue)) {
    return 'user';
  }

  return 'default';
}

function valuesEqual(left: unknown, right: unknown): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}

function cloneServerConfig(config: ServerConfig): ServerConfig {
  return {
    ...config,
    diagnostics: { ...config.diagnostics },
    formatting: { ...config.formatting },
    imports: { ...config.imports },
    semanticTokens: { ...config.semanticTokens },
    completion: { ...config.completion },
    inlayHints: { ...config.inlayHints },
    evaluation: { ...config.evaluation },
    allowedRoots: [...config.allowedRoots],
    allowedSchemes: [...config.allowedSchemes],
    excludeGlobs: [...config.excludeGlobs],
  };
}

function isWorkspaceScoped(scope: ConfigurationScope): boolean {
  return scope === 'workspace' || scope === 'workspaceFolder';
}

function broadensAllowedSchemes(allowedSchemes: readonly string[]): boolean {
  return allowedSchemes.some((scheme) => scheme !== 'file');
}
