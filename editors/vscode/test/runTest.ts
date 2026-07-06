import * as fs from 'node:fs/promises';
import * as os from 'node:os';
import * as path from 'node:path';
import { runTests } from '@vscode/test-electron';

async function main(): Promise<void> {
  try {
    const extensionDevelopmentPath = path.resolve(__dirname, '..', '..');
    const extensionTestsPath = path.resolve(__dirname, 'suite', 'index.js');
    const workspacePath = await createTestWorkspace(extensionDevelopmentPath);
    const userDataDir = await fs.mkdtemp(path.join(os.tmpdir(), 'lymals-vscode-userdata-'));
    const serverPath = await resolveRepositoryDebugServerPath(extensionDevelopmentPath);

    await runTests({
      extensionDevelopmentPath,
      extensionTestsPath,
      extensionTestsEnv: {
        TEST_LYMALS_WORKSPACE: workspacePath,
        TEST_LYMALS_SERVER_PATH: serverPath ?? '',
      },
      launchArgs: [
        workspacePath,
        '--user-data-dir',
        userDataDir,
      ],
    });
  } catch (error) {
    console.error('Failed to run VS Code extension tests.');
    console.error('Setup notes: tests never require a globally installed lymals binary.');
    console.error('A repository-local target/debug/lymals(.exe) binary is used only for the optional file-backed activation smoke test; otherwise that test is skipped.');
    console.error('If VS Code reports profile or mutex conflicts, close other extension-test instances and rerun npm test.');
    console.error(error);
    process.exit(1);
  }
}

void main();

async function createTestWorkspace(extensionDevelopmentPath: string): Promise<string> {
  const workspacePath = await fs.mkdtemp(path.join(os.tmpdir(), 'lymals-vscode-workspace-'));
  await fs.mkdir(path.join(workspacePath, '.vscode'), { recursive: true });
  await fs.writeFile(
    path.join(workspacePath, '.vscode', 'settings.json'),
    JSON.stringify(
      {
        'security.workspace.trust.enabled': false,
      },
      null,
      2,
    ),
    'utf8',
  );

  return workspacePath;
}

async function resolveRepositoryDebugServerPath(extensionDevelopmentPath: string): Promise<string | undefined> {
  const candidate = path.resolve(
    extensionDevelopmentPath,
    '..',
    '..',
    'target',
    'debug',
    process.platform === 'win32' ? 'lymals.exe' : 'lymals',
  );

  try {
    await fs.access(candidate);
    return candidate;
  } catch {
    return undefined;
  }
}
