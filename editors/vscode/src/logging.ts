import * as vscode from 'vscode';
import { LogLevel } from './config';

export const OUTPUT_CHANNEL_NAME = 'Lymals';

const levelWeight: Record<LogLevel, number> = {
  debug: 10,
  info: 20,
  warn: 30,
  error: 40,
};

export interface Logger {
  readonly channel: vscode.OutputChannel;
  show(preserveFocus?: boolean): void;
  setLevel(level: LogLevel): void;
  debug(message: string): void;
  info(message: string): void;
  warn(message: string): void;
  error(message: string): void;
  dispose(): void;
}

export function createLogger(level: LogLevel): Logger {
  const channel = vscode.window.createOutputChannel(OUTPUT_CHANNEL_NAME);
  let activeLevel = level;

  const log = (messageLevel: LogLevel, message: string): void => {
    if (levelWeight[messageLevel] < levelWeight[activeLevel]) {
      return;
    }

    channel.appendLine(`[${new Date().toISOString()}] [${messageLevel.toUpperCase()}] ${message}`);
  };

  return {
    channel,
    show: (preserveFocus?: boolean) => channel.show(preserveFocus),
    setLevel: (level: LogLevel) => {
      activeLevel = level;
    },
    debug: (message: string) => log('debug', message),
    info: (message: string) => log('info', message),
    warn: (message: string) => log('warn', message),
    error: (message: string) => log('error', message),
    dispose: () => channel.dispose(),
  };
}
