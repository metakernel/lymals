import * as vscode from 'vscode';
import { ExecuteCommandRequest, LanguageClient } from 'vscode-languageclient/node';
import { Logger } from './logging';

export interface CommandDependencies {
  logger: Logger;
  restart(): Thenable<void>;
  getClient(document?: vscode.TextDocument): Promise<LanguageClient>;
}

const COMMAND_PREVIEW_SCHEME = 'lymals-command';
const SERVER_COMMANDS = {
  restartIndex: 'lymals.restartIndex',
  showSyntaxTree: 'lymals.showSyntaxTree',
  showConfig: 'lymals.showConfig',
  formatWorkspaceFile: 'lymals.formatWorkspaceFile',
  explainDiagnostic: 'lymals.explainDiagnostic',
} as const;

export function registerCommands(context: vscode.ExtensionContext, dependencies: CommandDependencies): void {
  const previewDocuments = new CommandPreviewDocuments();

  context.subscriptions.push(
    previewDocuments,
    vscode.workspace.registerTextDocumentContentProvider(COMMAND_PREVIEW_SCHEME, previewDocuments),
    vscode.commands.registerCommand('lymals.restartLanguageServer', async () => {
      dependencies.logger.info('Restart command invoked.');
      try {
        await dependencies.restart();
        await vscode.window.showInformationMessage('Lymals language server restarted.');
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        dependencies.logger.error(`Restart failed: ${message}`);
        await vscode.window.showErrorMessage(`Failed to restart Lymals language server: ${message}`);
      }
    }),
    vscode.commands.registerCommand('lymals.showOutput', () => {
      dependencies.logger.show(false);
    }),
    vscode.commands.registerCommand('lymals.vscode.restartIndex', async () => {
      await runServerCommand({
        commandId: SERVER_COMMANDS.restartIndex,
        title: 'Restart Index',
        dependencies,
        onSuccess: async (result) => {
          const content = toDisplayText(result);
          appendCommandResult(dependencies.logger, 'Restart Index', content);
          dependencies.logger.show(true);
          await vscode.window.showInformationMessage(summarizeResult('Lymals index restarted.', content));
        },
      });
    }),
    vscode.commands.registerCommand('lymals.vscode.showSyntaxTree', async () => {
      const editor = getActiveLymaEditor({ requireFile: false });
      if (!editor) {
        return;
      }

      await runServerCommand({
        commandId: SERVER_COMMANDS.showSyntaxTree,
        title: 'Show Syntax Tree',
        dependencies,
        document: editor.document,
        arguments: [{ uri: editor.document.uri.toString() }],
        onSuccess: async (result) => {
          await openPreviewDocument(previewDocuments, {
            title: `Syntax Tree: ${editor.document.fileName || editor.document.uri.path}`,
            content: toDisplayText(result),
            extension: 'txt',
            viewColumn: vscode.ViewColumn.Beside,
          });
        },
      });
    }),
    vscode.commands.registerCommand('lymals.vscode.showConfig', async () => {
      await runServerCommand({
        commandId: SERVER_COMMANDS.showConfig,
        title: 'Show Config',
        dependencies,
        onSuccess: async (result) => {
          await openPreviewDocument(previewDocuments, {
            title: 'Lymals Config',
            content: toDisplayText(result),
            extension: typeof result === 'string' ? 'txt' : 'json',
          });
        },
      });
    }),
    vscode.commands.registerCommand('lymals.vscode.formatWorkspaceFilePreview', async () => {
      const editor = getActiveLymaEditor({ requireFile: true });
      if (!editor) {
        return;
      }

      await runServerCommand({
        commandId: SERVER_COMMANDS.formatWorkspaceFile,
        title: 'Format Workspace File Preview',
        dependencies,
        document: editor.document,
        arguments: [{ uri: editor.document.uri.toString() }],
        onSuccess: async (result) => {
          await openPreviewDocument(previewDocuments, {
            title: `Format Preview: ${editor.document.fileName}`,
            content: toDisplayText(result),
            extension: 'lyma',
            viewColumn: vscode.ViewColumn.Beside,
          });
        },
      });
    }),
    vscode.commands.registerCommand('lymals.vscode.explainDiagnostic', async () => {
      const editor = getActiveLymaEditorIfAny();
      const diagnosticCode = await resolveDiagnosticCode(editor);
      if (!diagnosticCode) {
        return;
      }

      await runServerCommand({
        commandId: SERVER_COMMANDS.explainDiagnostic,
        title: 'Explain Diagnostic',
        dependencies,
        document: editor?.document,
        arguments: [{ code: diagnosticCode }],
        onSuccess: async (result) => {
          const content = toDisplayText(result);
          appendCommandResult(dependencies.logger, `Explain Diagnostic (${diagnosticCode})`, content);
          await vscode.window.showInformationMessage(summarizeResult(`Diagnostic ${diagnosticCode}:`, content));
        },
      });
    }),
  );
}

interface RunServerCommandOptions {
  commandId: string;
  title: string;
  dependencies: CommandDependencies;
  document?: vscode.TextDocument;
  arguments?: unknown[];
  onSuccess(result: unknown): Thenable<void>;
}

interface OpenPreviewOptions {
  title: string;
  content: string;
  extension: string;
  viewColumn?: vscode.ViewColumn;
}

class CommandPreviewDocuments implements vscode.TextDocumentContentProvider, vscode.Disposable {
  private readonly contents = new Map<string, string>();
  private readonly emitter = new vscode.EventEmitter<vscode.Uri>();

  public readonly onDidChange = this.emitter.event;

  public provideTextDocumentContent(uri: vscode.Uri): string {
    return this.contents.get(uri.toString()) ?? '';
  }

  public dispose(): void {
    this.contents.clear();
    this.emitter.dispose();
  }

  public publish(title: string, content: string, extension: string): vscode.Uri {
    const slug = title
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-+|-+$/g, '') || 'preview';
    const uri = vscode.Uri.from({
      scheme: COMMAND_PREVIEW_SCHEME,
      path: `/${slug}-${Date.now()}.${extension}`,
    });
    this.contents.set(uri.toString(), content);
    this.emitter.fire(uri);
    return uri;
  }
}

async function runServerCommand(options: RunServerCommandOptions): Promise<void> {
  const { commandId, title, dependencies, document, arguments: args = [], onSuccess } = options;
  dependencies.logger.info(`${title} command invoked.`);

  try {
    const client = await dependencies.getClient(document);
    const result = await client.sendRequest(ExecuteCommandRequest.type, {
      command: commandId,
      arguments: args,
    });
    await onSuccess(result);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    dependencies.logger.error(`${title} failed: ${message}`);
    await vscode.window.showErrorMessage(`Lymals ${title} failed: ${message}`);
  }
}

async function openPreviewDocument(provider: CommandPreviewDocuments, options: OpenPreviewOptions): Promise<void> {
  const uri = provider.publish(options.title, options.content, options.extension);
  let document = await vscode.workspace.openTextDocument(uri);
  const languageId = toPreviewLanguage(options.extension);
  if (languageId && document.languageId !== languageId) {
    document = await vscode.languages.setTextDocumentLanguage(document, languageId);
  }
  await vscode.window.showTextDocument(document, {
    preview: true,
    preserveFocus: false,
    viewColumn: options.viewColumn,
  });
}

function getActiveLymaEditor(options: { requireFile: boolean }): vscode.TextEditor | undefined {
  const editor = vscode.window.activeTextEditor;
  const document = editor?.document;
  if (!editor || !document || document.languageId !== 'lyma') {
    void vscode.window.showWarningMessage('Open an active Lyma editor to run this Lymals command.');
    return undefined;
  }

  if (options.requireFile && document.uri.scheme !== 'file') {
    void vscode.window.showWarningMessage('This Lymals command requires an active workspace-backed .lyma file.');
    return undefined;
  }

  return editor;
}

function getActiveLymaEditorIfAny(): vscode.TextEditor | undefined {
  const editor = vscode.window.activeTextEditor;
  return editor?.document.languageId === 'lyma' ? editor : undefined;
}

async function resolveDiagnosticCode(editor: vscode.TextEditor | undefined): Promise<string | undefined> {
  const diagnosticCode = editor ? findDiagnosticCode(editor) : undefined;
  if (diagnosticCode) {
    return diagnosticCode;
  }

  const input = await vscode.window.showInputBox({
    prompt: 'Enter a Lymals diagnostic code to explain',
    placeHolder: 'L003',
    validateInput: (value) => (value.trim().length > 0 ? undefined : 'Diagnostic code is required.'),
  });

  return input?.trim() || undefined;
}

function findDiagnosticCode(editor: vscode.TextEditor): string | undefined {
  const diagnostics = vscode.languages.getDiagnostics(editor.document.uri);
  const activeLine = editor.selection.active.line;
  const preferredDiagnostic = diagnostics.find((diagnostic) => diagnostic.range.contains(editor.selection.active))
    ?? diagnostics.find((diagnostic) => diagnostic.range.start.line === activeLine)
    ?? diagnostics.find((diagnostic) => diagnostic.code !== undefined);

  return preferredDiagnostic ? toDiagnosticCode(preferredDiagnostic.code) : undefined;
}

function toDiagnosticCode(code: vscode.Diagnostic['code']): string | undefined {
  if (typeof code === 'string' || typeof code === 'number') {
    return String(code);
  }

  if (code && typeof code === 'object' && 'value' in code) {
    return String(code.value);
  }

  return undefined;
}

function toDisplayText(result: unknown): string {
  if (result === undefined) {
    return '';
  }

  if (typeof result === 'string') {
    return result;
  }

  return JSON.stringify(result, null, 2) ?? '';
}

function summarizeResult(prefix: string, content: string): string {
  const firstLine = content.split(/\r?\n/u, 1)[0]?.trim();
  return firstLine ? `${prefix} ${firstLine}` : prefix;
}

function appendCommandResult(logger: Logger, title: string, content: string): void {
  logger.channel.appendLine(`=== ${title} ===`);
  for (const line of content.split(/\r?\n/u)) {
    logger.channel.appendLine(line);
  }
  logger.channel.appendLine('');
}

function toPreviewLanguage(extension: string): string | undefined {
  switch (extension) {
    case 'json':
      return 'json';
    case 'lyma':
      return 'lyma';
    default:
      return undefined;
  }
}
