import { execFile } from 'node:child_process';
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import * as vscode from 'vscode';
import type { Executable } from 'vscode-languageclient/node';
import { BuildTaskConfig, ExtensionConfig } from './config';

const ENV_SERVER_PATH = 'LYMALS_SERVER_PATH';
const VERSION_FLAG = '--version';
const VERSION_PATTERN = /(^|\s)lymals(\s|$)/i;
const SERVER_NAME = 'lymals';
const confirmedWorkspaceServerPaths = new Set<string>();

export interface ResolveServerExecutableOptions {
  workspaceFolder?: vscode.WorkspaceFolder;
  extensionRootPath?: string;
  environment?: NodeJS.ProcessEnv;
  platform?: NodeJS.Platform;
  isTrustedWorkspace?: boolean;
  confirmWorkspacePath?(resolvedPath: string): Promise<boolean>;
  showErrorMessage?(message: string): Thenable<string | undefined>;
  probeExecutable?(command: string, args: readonly string[]): Promise<string>;
  pathExists?(candidatePath: string): Promise<boolean>;
  runBuildTask?(build: BuildTaskConfig, workspaceFolder?: vscode.WorkspaceFolder): Promise<void>;
}

export async function resolveServerExecutable(
  config: ExtensionConfig,
  options: ResolveServerExecutableOptions = {},
): Promise<Executable> {
  const dependencies = withDefaultOptions(options);
  const baseExecutable = {
    args: [...config.serverArgs],
    options: {},
  } satisfies Omit<Executable, 'command'>;

  if (config.serverPath) {
    const command = await resolveConfiguredServerPath(config, dependencies);
    return {
      command,
      ...baseExecutable,
    };
  }

  const environmentServerPath = normalizeOptionalString(dependencies.environment[ENV_SERVER_PATH]);
  if (environmentServerPath) {
    const command = await validateCandidate(
      normalizeCandidatePath(environmentServerPath, dependencies.platform),
      'environment override',
      dependencies,
      createActionableFix('Update or unset LYMALS_SERVER_PATH.'),
    );

    return {
      command,
      ...baseExecutable,
    };
  }

  const developmentBinaryPath = await resolveDevelopmentBinary(dependencies);
  if (developmentBinaryPath) {
    const command = await validateCandidate(
      developmentBinaryPath,
      'repository development binary',
      dependencies,
      createActionableFix('Rebuild the repository debug binary at target/debug/lymals or remove the stale file.'),
    );

    return {
      command,
      ...baseExecutable,
    };
  }

  try {
    const command = await validateCandidate(
      SERVER_NAME,
      'PATH lookup',
      dependencies,
      createActionableFix('Install lymals on PATH, configure lymalsExtension.server.path, or set LYMALS_SERVER_PATH.'),
    );

    return {
      command,
      ...baseExecutable,
    };
  } catch (error) {
    if (shouldRunBuild(config, dependencies)) {
      await dependencies.runBuildTask(config.build, dependencies.workspaceFolder);

      const builtDevelopmentBinaryPath = await resolveDevelopmentBinary(dependencies);
      if (builtDevelopmentBinaryPath) {
        const command = await validateCandidate(
          builtDevelopmentBinaryPath,
          'built repository development binary',
          dependencies,
          createActionableFix('Check lymalsExtension.server.build.* settings or rebuild the debug binary.'),
        );

        return {
          command,
          ...baseExecutable,
        };
      }
    }

    throw error;
  }
}

async function resolveConfiguredServerPath(
  config: ExtensionConfig,
  options: RequiredResolveServerExecutableOptions,
): Promise<string> {
  const configuredPath = config.serverPath;
  if (!configuredPath) {
    throw new Error('Missing configured server path.');
  }

  if (config.serverPathScope === 'workspace' || config.serverPathScope === 'workspaceFolder') {
    if (!options.isTrustedWorkspace) {
      const message = [
        'Refusing to use workspace-scoped lymalsExtension.server.path in an untrusted workspace.',
        'Trust this workspace or move lymalsExtension.server.path to user settings.',
      ].join(' ');
      await options.showErrorMessage(message);
      throw new Error(message);
    }

    const resolvedPath = resolveCommandPath(configuredPath, options.workspaceFolder);
    const normalizedResolvedPath = normalizeCandidatePath(resolvedPath, options.platform);
    if (!confirmedWorkspaceServerPaths.has(normalizedResolvedPath)) {
      const approved = await options.confirmWorkspacePath(normalizedResolvedPath);
      if (!approved) {
        const message = `Lymals server start canceled because workspace-scoped path was not approved: ${normalizedResolvedPath}. Fix: approve the prompt or move lymalsExtension.server.path to user settings.`;
        await options.showErrorMessage(message);
        throw new Error(message);
      }

      confirmedWorkspaceServerPaths.add(normalizedResolvedPath);
    }

    return validateCandidate(
      normalizedResolvedPath,
      'workspace-scoped setting',
      options,
      createActionableFix('Update lymalsExtension.server.path in workspace settings or move it to user settings.'),
    );
  }

  const resolvedPath = resolveCommandPath(configuredPath, options.workspaceFolder);
  return validateCandidate(
    normalizeCandidatePath(resolvedPath, options.platform),
    'configured setting',
    options,
    createActionableFix('Update lymalsExtension.server.path or remove it to allow fallback resolution.'),
  );
}

function resolveCommandPath(serverPath: string, workspaceFolder?: vscode.WorkspaceFolder): string {
  if (path.isAbsolute(serverPath)) {
    return serverPath;
  }

  if (workspaceFolder) {
    return path.resolve(workspaceFolder.uri.fsPath, serverPath);
  }

  return path.resolve(serverPath);
}

function normalizeCandidatePath(candidatePath: string, platform: NodeJS.Platform): string {
  if (platform !== 'win32' || path.extname(candidatePath).toLowerCase() === '.exe') {
    return candidatePath;
  }

  if (candidatePath.includes(path.sep) || path.isAbsolute(candidatePath)) {
    return `${candidatePath}.exe`;
  }

  return candidatePath;
}

async function resolveDevelopmentBinary(options: RequiredResolveServerExecutableOptions): Promise<string | undefined> {
  const candidate = normalizeCandidatePath(
    path.resolve(options.extensionRootPath, '..', '..', 'target', 'debug', SERVER_NAME),
    options.platform,
  );

  return (await options.pathExists(candidate)) ? candidate : undefined;
}

async function validateCandidate(
  command: string,
  sourceLabel: string,
  options: RequiredResolveServerExecutableOptions,
  fix: string,
): Promise<string> {
  try {
    const versionOutput = (await options.probeExecutable(command, [VERSION_FLAG])).trim();
    if (!VERSION_PATTERN.test(versionOutput)) {
      throw new Error(
        `Resolved ${sourceLabel} at ${command} is not a lymals server (received version output: ${JSON.stringify(versionOutput)}). ${fix}`,
      );
    }

    return command;
  } catch (error) {
    throw new Error(formatProbeFailure(command, sourceLabel, error, fix));
  }
}

function formatProbeFailure(command: string, sourceLabel: string, error: unknown, fix: string): string {
  const message = error instanceof Error ? error.message : String(error);
  if (message.includes('is not a lymals server')) {
    return message;
  }

  return `Failed to start lymals from ${sourceLabel} (${command}). ${message}. ${fix}`;
}

function shouldRunBuild(config: ExtensionConfig, options: RequiredResolveServerExecutableOptions): boolean {
  return Boolean(
    config.buildOnActivation
      && config.build.command
      && options.isTrustedWorkspace,
  );
}

function createActionableFix(fix: string): string {
  return `Fix: ${fix}`;
}

function withDefaultOptions(options: ResolveServerExecutableOptions): RequiredResolveServerExecutableOptions {
  return {
    workspaceFolder: options.workspaceFolder,
    extensionRootPath: options.extensionRootPath ?? path.resolve(__dirname, '..', '..'),
    environment: options.environment ?? process.env,
    platform: options.platform ?? process.platform,
    isTrustedWorkspace: options.isTrustedWorkspace ?? vscode.workspace.isTrusted,
    confirmWorkspacePath: options.confirmWorkspacePath ?? defaultConfirmWorkspacePath,
    showErrorMessage: options.showErrorMessage ?? vscode.window.showErrorMessage,
    probeExecutable: options.probeExecutable ?? defaultProbeExecutable,
    pathExists: options.pathExists ?? defaultPathExists,
    runBuildTask: options.runBuildTask ?? defaultRunBuildTask,
  };
}

async function defaultConfirmWorkspacePath(resolvedPath: string): Promise<boolean> {
  const useNow = 'Use Server';
  const selection = await vscode.window.showWarningMessage(
    `This workspace configured the lymals server executable at:\n${resolvedPath}\n\nOnly continue if you trust this path.`,
    { modal: true },
    useNow,
  );

  return selection === useNow;
}

async function defaultProbeExecutable(command: string, args: readonly string[]): Promise<string> {
  return await new Promise<string>((resolve, reject) => {
    execFile(command, [...args], { shell: false, windowsHide: true }, (error, stdout, stderr) => {
      if (error) {
        const details = stderr.trim() || stdout.trim() || error.message;
        reject(new Error(details));
        return;
      }

      const output = stdout.trim() || stderr.trim();
      if (!output) {
        reject(new Error('No version output returned by executable.'));
        return;
      }

      resolve(output);
    });
  });
}

async function defaultPathExists(candidatePath: string): Promise<boolean> {
  try {
    await fs.access(candidatePath);
    return true;
  } catch {
    return false;
  }
}

async function defaultRunBuildTask(build: BuildTaskConfig, workspaceFolder?: vscode.WorkspaceFolder): Promise<void> {
  const command = build.command;
  if (!command) {
    return;
  }

  const execution = await new Promise<void>((resolve, reject) => {
    const child = execFile(
      command,
      build.args,
      {
        cwd: resolveBuildCwd(build.cwd, workspaceFolder),
        shell: false,
        windowsHide: true,
      },
      (error: Error | null, stdout: string | Buffer, stderr: string | Buffer) => {
        if (error) {
          const details = stderr.toString().trim() || stdout.toString().trim() || error.message;
          reject(new Error(`Build task failed: ${details}`));
          return;
        }

        resolve();
      },
    );

    child.on('error', (error) => reject(error));
  });

  return execution;
}

function resolveBuildCwd(buildCwd: string | undefined, workspaceFolder?: vscode.WorkspaceFolder): string | undefined {
  if (!buildCwd) {
    return workspaceFolder?.uri.fsPath;
  }

  if (path.isAbsolute(buildCwd)) {
    return buildCwd;
  }

  return workspaceFolder ? path.resolve(workspaceFolder.uri.fsPath, buildCwd) : path.resolve(buildCwd);
}

function normalizeOptionalString(value: string | undefined): string | undefined {
  const trimmed = value?.trim() ?? '';
  return trimmed.length > 0 ? trimmed : undefined;
}

interface RequiredResolveServerExecutableOptions {
  workspaceFolder?: vscode.WorkspaceFolder;
  extensionRootPath: string;
  environment: NodeJS.ProcessEnv;
  platform: NodeJS.Platform;
  isTrustedWorkspace: boolean;
  confirmWorkspacePath(resolvedPath: string): Promise<boolean>;
  showErrorMessage(message: string): Thenable<string | undefined>;
  probeExecutable(command: string, args: readonly string[]): Promise<string>;
  pathExists(candidatePath: string): Promise<boolean>;
  runBuildTask(build: BuildTaskConfig, workspaceFolder?: vscode.WorkspaceFolder): Promise<void>;
}
