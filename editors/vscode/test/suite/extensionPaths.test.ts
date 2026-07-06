import * as assert from 'node:assert/strict';
import * as path from 'node:path';
import * as vscode from 'vscode';
import { getDefaultDevelopmentLogFilePath, resolveServerLogFilePath } from '../../src/extension';

suite('extension path helpers', () => {
  test('resolves workspace-relative log files with Windows path semantics', () => {
    const workspaceFolder = createWorkspaceFolder('C:\\Users\\dev\\My Project', 'My Project');

    const resolved = resolveServerLogFilePath('logs\\lymals.log', workspaceFolder, path.win32);

    assert.equal(resolved, 'C:\\Users\\dev\\My Project\\logs\\lymals.log');
  });

  test('preserves absolute Unix log files with spaces', () => {
    const workspaceFolder = createWorkspaceFolder('/repo/workspace', 'workspace');

    const resolved = resolveServerLogFilePath('/var/tmp/my logs/lymals.log', workspaceFolder, path.posix);

    assert.equal(resolved, '/var/tmp/my logs/lymals.log');
  });

  test('builds the default development log file under storage for Windows workspaces', () => {
    const workspaceFolder = createWorkspaceFolder('C:\\repo\\workspace', 'workspace');

    const resolved = getDefaultDevelopmentLogFilePath(
      vscode.ExtensionMode.Development,
      'C:\\Users\\dev\\AppData\\Roaming\\Code\\User\\workspaceStorage\\123',
      workspaceFolder,
      path.win32,
    );

    assert.equal(
      resolved,
      'C:\\Users\\dev\\AppData\\Roaming\\Code\\User\\workspaceStorage\\123\\logs\\workspace.lymals.log',
    );
  });

  test('builds the default development log file under storage for Unix workspaces', () => {
    const workspaceFolder = createWorkspaceFolder('/repo/my app', 'my app');

    const resolved = getDefaultDevelopmentLogFilePath(
      vscode.ExtensionMode.Development,
      '/home/dev/.config/Code/User/workspaceStorage/123',
      workspaceFolder,
      path.posix,
    );

    assert.equal(resolved, '/home/dev/.config/Code/User/workspaceStorage/123/logs/my app.lymals.log');
  });

  test('does not create a default development log file outside development mode', () => {
    const resolved = getDefaultDevelopmentLogFilePath(
      vscode.ExtensionMode.Production,
      '/home/dev/.config/Code/User/workspaceStorage/123',
      createWorkspaceFolder('/repo/workspace', 'workspace'),
      path.posix,
    );

    assert.equal(resolved, undefined);
  });
});

function createWorkspaceFolder(fsPath: string, name: string): vscode.WorkspaceFolder {
  return {
    uri: { fsPath } as vscode.Uri,
    name,
    index: 0,
  };
}
