import * as assert from 'node:assert/strict';
import * as path from 'node:path';
import * as vscode from 'vscode';

suite('Lymals VS Code extension scaffold', () => {
  const extensionId = 'local-dev.lymals-vscode';
  const commandIds = [
    'lymals.restartLanguageServer',
    'lymals.showOutput',
    'lymals.vscode.restartIndex',
    'lymals.vscode.showSyntaxTree',
    'lymals.vscode.showConfig',
    'lymals.vscode.formatWorkspaceFilePreview',
    'lymals.vscode.explainDiagnostic',
  ] as const;

  test('registers extension metadata', () => {
    const extension = vscode.extensions.getExtension(extensionId);

    assert.ok(extension);
    assert.equal(extension?.packageJSON.main, './out/extension.js');
  });

  test('declares the expected configuration schema', () => {
    const extension = vscode.extensions.getExtension(extensionId);
    assert.ok(extension);

    const properties = extension.packageJSON.contributes?.configuration?.properties as Record<string, {
      type?: string;
      default?: unknown;
      items?: { type?: string };
      enum?: string[];
      restricted?: boolean;
      markdownDescription?: string;
    }>;

    assert.ok(properties);
    assert.equal(properties['lymalsExtension.server.path']?.type, 'string');
    assert.equal(properties['lymalsExtension.server.path']?.default, '');
    assert.match(properties['lymalsExtension.server.path']?.markdownDescription ?? '', /Leave empty to fall back to LYMALS_SERVER_PATH/);
    assert.equal(properties['lymalsExtension.server.args']?.type, 'array');
    assert.equal(properties['lymalsExtension.server.args']?.items?.type, 'string');
    assert.equal(properties['lymalsExtension.server.allowUntitled']?.default, false);
    assert.deepEqual(properties['lymalsExtension.server.trace.server']?.enum, ['off', 'messages', 'verbose']);
    assert.deepEqual(properties['lymals.allowedSchemes']?.default, ['file']);
    assert.equal(properties['lymals.evaluation.enabled']?.restricted, true);
  });

  test('activates when a Lyma document is opened', async function () {
    this.timeout(20000);

    const extension = vscode.extensions.getExtension(extensionId);
    assert.ok(extension);

    const document = await vscode.workspace.openTextDocument({
      language: 'lyma',
      content: '# activation smoke test\n---\nname: test\n',
    });
    await vscode.window.showTextDocument(document);

    await waitFor(() => Boolean(vscode.extensions.getExtension(extensionId)?.isActive), 'extension activation after opening a Lyma document');
    assert.equal(vscode.extensions.getExtension(extensionId)?.isActive, true);
  });

  test('registers the public commands after activation', async function () {
    this.timeout(20000);

    const extension = vscode.extensions.getExtension(extensionId);
    assert.ok(extension);
    if (!extension.isActive) {
      await extension.activate();
    }

    const commands = await vscode.commands.getCommands(true);
    for (const commandId of commandIds) {
      assert.ok(commands.includes(commandId), `expected registered command ${commandId}`);
    }
  });

  test('can start against the repository debug server without requiring a global install', async function () {
    this.timeout(60000);

    const serverPath = process.env.TEST_LYMALS_SERVER_PATH;
    const workspacePath = process.env.TEST_LYMALS_WORKSPACE;
    if (process.env.TEST_ENABLE_REAL_SERVER !== '1' || !serverPath || !workspacePath) {
      this.skip();
      return;
    }

    const extensionConfig = vscode.workspace.getConfiguration('lymalsExtension');
    const previousServerPath = extensionConfig.get<string>('server.path');
    const previousAllowUntitled = extensionConfig.get<boolean>('server.allowUntitled');

    try {
      await extensionConfig.update('server.path', serverPath, vscode.ConfigurationTarget.Global);
      await extensionConfig.update('server.allowUntitled', false, vscode.ConfigurationTarget.Global);

      const documentPath = path.join(workspacePath, 'sample.lyma');
      await vscode.workspace.fs.writeFile(
        vscode.Uri.file(documentPath),
        Buffer.from('# file-backed activation smoke test\n---\nmessage: hello\n', 'utf8'),
      );

      const document = await vscode.workspace.openTextDocument(documentPath);
      await vscode.window.showTextDocument(document);

      const extension = vscode.extensions.getExtension(extensionId);
      assert.ok(extension);
      if (!extension.isActive) {
        await extension.activate();
      }

      await vscode.commands.executeCommand('lymals.restartLanguageServer');
    } catch (error) {
      const instructions = [
        'Repository debug server activation test failed.',
        `Configured TEST_LYMALS_SERVER_PATH=${serverPath}.`,
        'Rebuild the repository debug binary (for example, cargo build -p lymals) or rerun npm test without TEST_LYMALS_SERVER_PATH to skip this integration check.',
      ].join(' ');
      assert.fail(`${instructions} Original error: ${error instanceof Error ? error.message : String(error)}`);
    } finally {
      await extensionConfig.update('server.path', previousServerPath ?? '', vscode.ConfigurationTarget.Global);
      await extensionConfig.update('server.allowUntitled', previousAllowUntitled ?? false, vscode.ConfigurationTarget.Global);
    }
  });
});

async function waitFor(predicate: () => boolean | Promise<boolean>, label: string, timeoutMs = 15000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await predicate()) {
      return;
    }

    await new Promise((resolve) => setTimeout(resolve, 100));
  }

  assert.fail(`Timed out waiting for ${label}.`);
}
