import * as vscode from 'vscode';
import * as path from 'node:path';
import {
  ConfigurationItem,
  DocumentSelector,
  Executable,
  LanguageClient,
  State,
  ServerOptions,
  Trace,
  TransportKind,
} from 'vscode-languageclient/node';
import { registerCommands } from './commands';
import { EXTENSION_SECTION, getExtensionConfig, getServerConfig, SERVER_SECTION, TraceLevel } from './config';
import { createLogger, Logger } from './logging';
import { resolveServerExecutable } from './serverResolver';

let client: LanguageClient | undefined;
let logger: Logger | undefined;
let activationContext: vscode.ExtensionContext | undefined;
let clientWorkspaceFolder: vscode.WorkspaceFolder | undefined;
let clientStartPromise: Promise<void> | undefined;

const SERVER_RESTART_SETTINGS = [
  'server.path',
  'server.args',
  'server.allowUntitled',
  'server.logFile',
  'server.rustLog',
  'server.build.command',
  'server.build.args',
  'server.build.cwd',
  'server.buildOnActivation',
] as const;

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  activationContext = context;
  const config = getExtensionConfig(getPrimaryWorkspaceFolder());
  logger = createLogger(config.logLevel);
  context.subscriptions.push(logger);
  context.subscriptions.push({
    dispose: () => {
      void stopClient();
    },
  });

  logger.info('Activating Lymals VS Code extension scaffold. Waiting for an eligible .lyma document before starting the language client.');

  registerCommands(context, {
    logger,
    restart: async () => {
      await restartClient();
    },
    getClient: async (document?: vscode.TextDocument) => {
      if (document) {
        await ensureClientForDocument(document);
      } else {
        await ensureClientForCommand();
      }

      if (!client) {
        throw new Error('Open a .lyma file to start the Lymals language server before running this command.');
      }

      return client;
    },
  });

  context.subscriptions.push(
    vscode.workspace.onDidOpenTextDocument((document) => {
      void ensureClientForDocument(document);
    }),
    vscode.workspace.onDidChangeConfiguration(async (event) => {
      if (event.affectsConfiguration(SERVER_SECTION)) {
        pushServerConfiguration();
      }

      if (event.affectsConfiguration(EXTENSION_SECTION)) {
        const nextWorkspaceFolder = clientWorkspaceFolder ?? getPrimaryWorkspaceFolder();
        const nextConfig = getExtensionConfig(nextWorkspaceFolder);
        logger?.setLevel(nextConfig.logLevel);

        if (event.affectsConfiguration(`${EXTENSION_SECTION}.trace.client`)) {
          logger?.info(`Updated Lymals output verbosity to ${nextConfig.logLevel}.`);
        }

        if (event.affectsConfiguration(`${EXTENSION_SECTION}.server.trace.server`)) {
          try {
            await applyTraceSetting(client, nextConfig.serverTrace);
            logger?.info(
              client
                ? `Updated language server protocol trace to ${nextConfig.serverTrace}.`
                : `Stored language server protocol trace=${nextConfig.serverTrace}; it will apply when the client starts.`,
            );
          } catch (error) {
            reportStartupFailure(error);
          }
        }

        if (requiresClientRestart(event)) {
          logger?.info(
            'Configuration changed; restarting language client to apply updated server launch arguments/environment.',
          );
          try {
            await restartClient();
          } catch (error) {
            reportStartupFailure(error);
          }
        }
      }
    }),
    vscode.workspace.onDidGrantWorkspaceTrust(() => {
      logger?.info('Workspace trust granted; re-evaluating Lyma language client startup.');
      pushServerConfiguration();
      void ensureClientForOpenLymaDocuments();
    }),
  );

  await ensureClientForOpenLymaDocuments();
}

export async function deactivate(): Promise<void> {
  await stopClient();
  logger?.dispose();
  logger = undefined;
}

async function restartClient(): Promise<void> {
  await stopClient();
  await ensureClientForOpenLymaDocuments();
}

async function ensureClientForOpenLymaDocuments(): Promise<void> {
  for (const document of vscode.workspace.textDocuments) {
    if (shouldManageDocument(document)) {
      await ensureClientForDocument(document);
      return;
    }
  }
}

async function ensureClientForCommand(): Promise<void> {
  const activeDocument = vscode.window.activeTextEditor?.document;
  if (activeDocument && shouldManageDocument(activeDocument)) {
    await ensureClientForDocument(activeDocument);
    return;
  }

  await ensureClientForOpenLymaDocuments();
}

async function ensureClientForDocument(document: vscode.TextDocument): Promise<void> {
  if (!shouldManageDocument(document)) {
    return;
  }

  if (client) {
    return;
  }

  if (clientStartPromise) {
    await clientStartPromise;
    return;
  }

  const workspaceFolder = vscode.workspace.getWorkspaceFolder(document.uri) ?? getPrimaryWorkspaceFolder();
  clientWorkspaceFolder = workspaceFolder;
  clientStartPromise = startClient(workspaceFolder);

  try {
    await clientStartPromise;
  } finally {
    clientStartPromise = undefined;
  }
}

async function startClient(workspaceFolder?: vscode.WorkspaceFolder): Promise<void> {
  if (client) {
    return;
  }

  const config = getExtensionConfig(workspaceFolder);
  logger?.setLevel(config.logLevel);
  const executable = await resolveServerExecutable(config, { workspaceFolder });
  const executableWithRuntimeConfig = await applyServerRuntimeConfiguration(executable, config, workspaceFolder);

  const serverOptions: ServerOptions = {
    run: withStdioTransport(executableWithRuntimeConfig),
    debug: {
      ...executableWithRuntimeConfig,
      transport: TransportKind.stdio,
    },
  };

  const documentSelector: DocumentSelector = [{ scheme: 'file', language: 'lyma' }];
  if (config.allowUntitled) {
    documentSelector.push({ scheme: 'untitled', language: 'lyma' });
  }

  client = new LanguageClient(
    'lymals',
    'Lymals Language Server',
    serverOptions,
    {
      documentSelector,
      outputChannel: logger?.channel,
      traceOutputChannel: logger?.channel,
      middleware: {
        workspace: {
          configuration: async (params, _token, next) => {
            const response = await next(params, _token);
            const fallbackValues = Array.isArray(response) ? response : [];
            return params.items.map((item, index) => {
              if (item.section !== SERVER_SECTION) {
                return fallbackValues[index];
              }

              return getServerConfigurationForItem(item);
            });
          },
        },
      },
    },
  );

  await applyTraceSetting(client, config.serverTrace);
  activationContext?.subscriptions.push(
    client.onDidChangeState((event) => {
      logger?.info(`Language client state: ${formatClientState(event.oldState)} -> ${formatClientState(event.newState)}`);
    }),
  );
  logger?.info(`Starting server command: ${formatExecutableForLog(executableWithRuntimeConfig, workspaceFolder)}`);
  logServerLogFileLocation(executableWithRuntimeConfig, config.serverLogFile);

  try {
    await client.start();
    logger?.info('Language client start requested.');
  } catch (error) {
    client = undefined;
    throw error;
  }
}

function pushServerConfiguration(): void {
  if (!client) {
    return;
  }

  const serverConfig = getServerConfigurationForWorkspace(clientWorkspaceFolder);
  client.sendNotification('workspace/didChangeConfiguration', {
    settings: {
      [SERVER_SECTION]: serverConfig,
    },
  });
}

async function stopClient(): Promise<void> {
  if (!client) {
    return;
  }

  const currentClient = client;
  client = undefined;
  clientWorkspaceFolder = undefined;
  logger?.info('Stopping language client.');
  await currentClient.stop();
}

function shouldManageDocument(document: vscode.TextDocument): boolean {
  if (document.languageId !== 'lyma') {
    return false;
  }

  const workspaceFolder = vscode.workspace.getWorkspaceFolder(document.uri) ?? getPrimaryWorkspaceFolder();
  const config = getExtensionConfig(workspaceFolder);
  if (document.uri.scheme === 'file') {
    return true;
  }

  return document.uri.scheme === 'untitled' && config.allowUntitled;
}

async function applyServerRuntimeConfiguration(
  executable: Executable,
  config: ReturnType<typeof getExtensionConfig>,
  workspaceFolder?: vscode.WorkspaceFolder,
): Promise<Executable> {
  const args = [...(executable.args ?? [])];
  const options = { ...(executable.options ?? {}) };
  const env = { ...(options.env ?? process.env) };

  const resolvedLogFile = await resolveServerLogFile(config.serverLogFile, workspaceFolder);
  if (resolvedLogFile) {
    args.push('--log-file', resolvedLogFile);
  }

  if (config.rustLog) {
    env.RUST_LOG = config.rustLog;
  }

  return {
    ...executable,
    args,
    options: {
      ...options,
      env,
    },
  };
}

function withStdioTransport(executable: Executable): Executable {
  return {
    ...executable,
    transport: TransportKind.stdio,
  };
}

function getPrimaryWorkspaceFolder(): vscode.WorkspaceFolder | undefined {
  return vscode.workspace.workspaceFolders?.[0];
}

function resolveOptionalPath(candidatePath: string | undefined, workspaceFolder?: vscode.WorkspaceFolder): string | undefined {
  if (!candidatePath) {
    return undefined;
  }

  if (path.isAbsolute(candidatePath)) {
    return candidatePath;
  }

  if (workspaceFolder) {
    return path.resolve(workspaceFolder.uri.fsPath, candidatePath);
  }

  return path.resolve(candidatePath);
}

function formatExecutableForLog(executable: Executable, workspaceFolder?: vscode.WorkspaceFolder): string {
  const parts = [sanitizePathForLog(executable.command, workspaceFolder)];
  const args = executable.args ?? [];
  for (let index = 0; index < args.length; index += 1) {
    const value = args[index];
    if (value === '--log-file' && index + 1 < args.length) {
      parts.push(value, sanitizePathForLog(args[index + 1], workspaceFolder));
      index += 1;
      continue;
    }

    parts.push(value);
  }

  const rustLog = executable.options?.env?.RUST_LOG;
  if (rustLog) {
    parts.push(`(RUST_LOG=${rustLog})`);
  }

  return parts.join(' ');
}

function sanitizePathForLog(candidatePath: string, workspaceFolder?: vscode.WorkspaceFolder): string {
  const normalizedWorkspacePath = workspaceFolder?.uri.fsPath;
  if (normalizedWorkspacePath && candidatePath.startsWith(normalizedWorkspacePath)) {
    return `<workspace>/${path.relative(normalizedWorkspacePath, candidatePath).replace(/\\/g, '/')}`;
  }

  const homePath = process.env.USERPROFILE ?? process.env.HOME;
  if (homePath && candidatePath.startsWith(homePath)) {
    return `~/${path.relative(homePath, candidatePath).replace(/\\/g, '/')}`;
  }

  return candidatePath;
}

function formatClientState(state: State): string {
  switch (state) {
    case State.Starting:
      return 'starting';
    case State.Running:
      return 'running';
    case State.Stopped:
      return 'stopped';
    default:
      return String(state);
  }
}

async function applyTraceSetting(languageClient: LanguageClient | undefined, trace: TraceLevel): Promise<void> {
  if (!languageClient) {
    return;
  }

  await languageClient.setTrace(toProtocolTrace(trace));
}

function getServerConfigurationForItem(item: ConfigurationItem): ReturnType<typeof getServerConfigurationForWorkspace> {
  const workspaceFolder = getWorkspaceFolderForScopeUri(item.scopeUri);
  return getServerConfigurationForWorkspace(workspaceFolder);
}

function requiresClientRestart(event: vscode.ConfigurationChangeEvent): boolean {
  return SERVER_RESTART_SETTINGS.some((setting) => event.affectsConfiguration(`${EXTENSION_SECTION}.${setting}`));
}

function getServerConfigurationForWorkspace(workspaceFolder?: vscode.WorkspaceFolder) {
  const result = getServerConfig(workspaceFolder, { isTrustedWorkspace: vscode.workspace.isTrusted });
  for (const warning of result.warnings) {
    logger?.warn(warning);
  }

  return result.config;
}

function getWorkspaceFolderForScopeUri(scopeUri: string | vscode.Uri | undefined): vscode.WorkspaceFolder | undefined {
  if (!scopeUri) {
    return clientWorkspaceFolder ?? getPrimaryWorkspaceFolder();
  }

  const uri = typeof scopeUri === 'string' ? vscode.Uri.parse(scopeUri) : scopeUri;
  return vscode.workspace.getWorkspaceFolder(uri) ?? clientWorkspaceFolder ?? getPrimaryWorkspaceFolder();
}

function reportStartupFailure(error: unknown): void {
  const message = error instanceof Error ? error.message : String(error);
  logger?.error(`Language client startup failed: ${message}`);
  void vscode.window.showWarningMessage(`Lymals language server failed to start: ${message}`);
}

function toProtocolTrace(trace: TraceLevel): Trace {
  switch (trace) {
    case 'messages':
      return Trace.Messages;
    case 'verbose':
      return Trace.Verbose;
    case 'off':
    default:
      return Trace.Off;
  }
}

async function resolveServerLogFile(
  configuredPath: string | undefined,
  workspaceFolder?: vscode.WorkspaceFolder,
): Promise<string | undefined> {
  const resolvedPath = resolveOptionalPath(configuredPath, workspaceFolder) ?? getDefaultDevelopmentLogFile(workspaceFolder);
  if (!resolvedPath) {
    return undefined;
  }

  await vscode.workspace.fs.createDirectory(vscode.Uri.file(path.dirname(resolvedPath)));
  return resolvedPath;
}

function getDefaultDevelopmentLogFile(workspaceFolder?: vscode.WorkspaceFolder): string | undefined {
  if (activationContext?.extensionMode !== vscode.ExtensionMode.Development) {
    return undefined;
  }

  const storagePath = activationContext.storageUri?.fsPath ?? activationContext.globalStorageUri.fsPath;
  const fileName = workspaceFolder ? `${workspaceFolder.name}.lymals.log` : 'lymals.log';
  return path.join(storagePath, 'logs', fileName);
}

function logServerLogFileLocation(executable: Executable, configuredLogFile: string | undefined): void {
  const args = executable.args ?? [];
  const logFileFlagIndex = args.indexOf('--log-file');
  if (logFileFlagIndex < 0 || logFileFlagIndex + 1 >= args.length) {
    return;
  }

  const resolvedLogFile = args[logFileFlagIndex + 1];
  const sourceLabel = configuredLogFile ? 'configured' : 'development default';
  logger?.info(`Server log file (${sourceLabel}): ${resolvedLogFile}`);
  void vscode.window.setStatusBarMessage(`Lymals server log: ${resolvedLogFile}`, 10000);
  logger?.info('Client and protocol logs are available in Output > Lymals. Server log path changes apply after restart.');
}
