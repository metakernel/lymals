import * as assert from 'node:assert/strict';
import * as vscode from 'vscode';

suite('Lymals VS Code extension scaffold', () => {
  test('registers extension metadata', () => {
    const extension = vscode.extensions.getExtension('local-dev.lymals-vscode');

    assert.ok(extension);
    assert.equal(extension?.packageJSON.main, './out/extension.js');
  });
});
