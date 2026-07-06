import * as assert from 'node:assert/strict';
import * as path from 'node:path';
import * as vscode from 'vscode';
import { ExtensionConfig } from '../../src/config';
import { resolveServerExecutable } from '../../src/serverResolver';

suite('serverResolver', () => {
  const workspaceFolder = createWorkspaceFolder('D:\\repos\\lymals\\workspace');

  test('resolves an explicit configured path and probes without a shell string', async () => {
    const calls: Array<{ command: string; args: readonly string[] }> = [];
    const executable = await resolveServerExecutable(createConfig({ serverPath: 'C:\\tools\\lymals' }), {
      workspaceFolder,
      platform: 'win32',
      probeExecutable: async (command, args) => {
        calls.push({ command, args });
        return 'lymals 1.2.3';
      },
    });

    assert.equal(executable.command, 'C:\\tools\\lymals.exe');
    assert.deepEqual(executable.args, []);
    assert.deepEqual(calls, [{ command: 'C:\\tools\\lymals.exe', args: ['--version'] }]);
  });

  test('reports an actionable error for a missing configured path', async () => {
    await assert.rejects(
      () => resolveServerExecutable(createConfig({ serverPath: '/missing/lymals' }), {
        probeExecutable: async () => {
          throw new Error('ENOENT');
        },
      }),
      (error: unknown) => {
        assert.match(String(error), /Failed to start lymals from configured setting/);
        assert.match(String(error), /Update lymalsExtension\.server\.path or remove it to allow fallback resolution/);
        return true;
      },
    );
  });

  test('normalizes Windows development binaries to .exe', async () => {
    const executable = await resolveServerExecutable(createConfig(), {
      platform: 'win32',
      extensionRootPath: 'D:\\repos\\lymals\\editors\\vscode',
      pathExists: async (candidatePath) => candidatePath === 'D:\\repos\\lymals\\target\\debug\\lymals.exe',
      probeExecutable: async (command) => {
        assert.equal(command, 'D:\\repos\\lymals\\target\\debug\\lymals.exe');
        return 'lymals 0.1.0';
      },
    });

    assert.equal(executable.command, 'D:\\repos\\lymals\\target\\debug\\lymals.exe');
  });

  test('probes a configured Windows path with spaces via command/argv without manual quoting', async () => {
    const calls: Array<{ command: string; args: readonly string[] }> = [];

    const executable = await resolveServerExecutable(createConfig({ serverPath: 'C:\\Program Files\\Lymals\\lymals' }), {
      workspaceFolder,
      platform: 'win32',
      probeExecutable: async (command, args) => {
        calls.push({ command, args });
        return 'lymals 1.2.3';
      },
    });

    assert.equal(executable.command, 'C:\\Program Files\\Lymals\\lymals.exe');
    assert.deepEqual(calls, [{ command: 'C:\\Program Files\\Lymals\\lymals.exe', args: ['--version'] }]);
  });

  test('falls back to PATH lookup when no other candidate is available', async () => {
    const calls: Array<{ command: string; args: readonly string[] }> = [];
    const executable = await resolveServerExecutable(createConfig({ serverArgs: ['--log-level', 'debug'] }), {
      extensionRootPath: '/repo/editors/vscode',
      pathExists: async () => false,
      probeExecutable: async (command, args) => {
        calls.push({ command, args });
        return 'lymals 9.9.9';
      },
    });

    assert.equal(executable.command, 'lymals');
    assert.deepEqual(executable.args, ['--log-level', 'debug']);
    assert.deepEqual(calls, [{ command: 'lymals', args: ['--version'] }]);
  });

  test('prefers LYMALS_SERVER_PATH over repository and PATH fallbacks', async () => {
    const calls: Array<{ command: string; args: readonly string[] }> = [];

    const executable = await resolveServerExecutable(createConfig({ serverArgs: ['--log-level', 'debug'] }), {
      environment: { LYMALS_SERVER_PATH: '/opt/custom/lymals' },
      platform: 'linux',
      pathExists: async () => false,
      probeExecutable: async (command, args) => {
        calls.push({ command, args });
        return 'lymals 3.4.5';
      },
    });

    assert.equal(executable.command, '/opt/custom/lymals');
    assert.deepEqual(executable.args, ['--log-level', 'debug']);
    assert.deepEqual(calls, [{ command: '/opt/custom/lymals', args: ['--version'] }]);
  });

  test('keeps Unix executable names unchanged for override paths with spaces', async () => {
    const calls: Array<{ command: string; args: readonly string[] }> = [];

    const executable = await resolveServerExecutable(createConfig(), {
      environment: { LYMALS_SERVER_PATH: '/opt/my apps/lymals' },
      platform: 'linux',
      pathExists: async () => false,
      probeExecutable: async (command, args) => {
        calls.push({ command, args });
        return 'lymals 3.4.5';
      },
    });

    assert.equal(executable.command, '/opt/my apps/lymals');
    assert.deepEqual(calls, [{ command: '/opt/my apps/lymals', args: ['--version'] }]);
  });

  test('rejects binaries whose version output is not from lymals', async () => {
    await assert.rejects(
      () => resolveServerExecutable(createConfig({ serverPath: '/usr/local/bin/lymals' }), {
        probeExecutable: async () => 'totally-not-lymals 1.0.0',
      }),
      (error: unknown) => {
        assert.match(String(error), /is not a lymals server/);
        assert.match(String(error), /version output/);
        return true;
      },
    );
  });

  test('blocks workspace-scoped paths in untrusted workspaces before resolve or spawn', async () => {
    let prompted = false;
    let probed = false;
    const errors: string[] = [];

    await assert.rejects(
      () => resolveServerExecutable(createConfig({ serverPath: './alt-bin/lymals', serverPathScope: 'workspace' }), {
        workspaceFolder,
        isTrustedWorkspace: false,
        confirmWorkspacePath: async () => {
          prompted = true;
          return true;
        },
        showErrorMessage: async (message) => {
          errors.push(message);
          return undefined;
        },
        probeExecutable: async () => {
          probed = true;
          return 'lymals 1.0.0';
        },
      }),
      /Refusing to use workspace-scoped lymalsExtension\.server\.path in an untrusted workspace/,
    );

    assert.equal(prompted, false);
    assert.equal(probed, false);
    assert.equal(errors.length, 1);
  });

  test('prompts once for trusted workspace-scoped paths and uses the resolved absolute path', async () => {
    const prompts: string[] = [];
    const probes: string[] = [];
    const config = createConfig({ serverPath: './bin/lymals', serverPathScope: 'workspaceFolder' });

    const first = await resolveServerExecutable(config, {
      workspaceFolder,
      isTrustedWorkspace: true,
      platform: 'win32',
      confirmWorkspacePath: async (resolvedPath) => {
        prompts.push(resolvedPath);
        return true;
      },
      probeExecutable: async (command) => {
        probes.push(command);
        return 'lymals 1.0.0';
      },
    });

    const second = await resolveServerExecutable(config, {
      workspaceFolder,
      isTrustedWorkspace: true,
      platform: 'win32',
      confirmWorkspacePath: async (resolvedPath) => {
        prompts.push(`second:${resolvedPath}`);
        return true;
      },
      probeExecutable: async (command) => {
        probes.push(command);
        return 'lymals 1.0.0';
      },
    });

    const expectedPath = path.resolve(workspaceFolder.uri.fsPath, './bin/lymals.exe');
    assert.equal(first.command, expectedPath);
    assert.equal(second.command, expectedPath);
    assert.deepEqual(prompts, [expectedPath]);
    assert.deepEqual(probes, [expectedPath, expectedPath]);
  });

  test('surfaces cancellation when workspace path approval is denied', async () => {
    const errors: string[] = [];

    await assert.rejects(
      () => resolveServerExecutable(createConfig({ serverPath: './rejected-bin/lymals', serverPathScope: 'workspace' }), {
        workspaceFolder,
        isTrustedWorkspace: true,
        confirmWorkspacePath: async () => false,
        showErrorMessage: async (message) => {
          errors.push(message);
          return undefined;
        },
      }),
      /workspace-scoped path was not approved/,
    );

    assert.equal(errors.length, 1);
    assert.match(errors[0], /approve the prompt or move lymalsExtension\.server\.path to user settings/);
  });
});

function createConfig(overrides: Partial<ExtensionConfig> = {}): ExtensionConfig {
  return {
    serverPath: undefined,
    serverArgs: [],
    allowUntitled: false,
    serverLogFile: undefined,
    rustLog: undefined,
    serverPathScope: 'none',
    build: {
      command: undefined,
      args: [],
      cwd: undefined,
    },
    buildOnActivation: false,
    serverTrace: 'off',
    logLevel: 'info',
    ...overrides,
  };
}

function createWorkspaceFolder(fsPath: string): vscode.WorkspaceFolder {
  return {
    uri: vscode.Uri.file(fsPath),
    name: path.basename(fsPath),
    index: 0,
  };
}
