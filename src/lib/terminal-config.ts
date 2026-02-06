import type { ITheme, ITerminalOptions } from '@xterm/xterm';

export const TERMINAL_BG = '#1a1a1e';

export const terminalTheme: ITheme = {
	background: TERMINAL_BG,
	foreground: '#dcdcde',
	cursor: '#c4b5fd',
	selectionBackground: '#7c3aed33',
	black: '#1a1a1e',
	red: '#f87171',
	green: '#4ade80',
	yellow: '#fbbf24',
	blue: '#93a3f8',
	magenta: '#c4b5fd',
	cyan: '#67e8f9',
	white: '#e4e4e7',
	brightBlack: '#63636e',
	brightRed: '#fca5a5',
	brightGreen: '#86efac',
	brightYellow: '#fde68a',
	brightBlue: '#b4bffc',
	brightMagenta: '#ddd6fe',
	brightCyan: '#a5f3fc',
	brightWhite: '#fafafa'
};

export const terminalOptions: ITerminalOptions = {
	allowTransparency: false,
	convertEol: false,
	cursorBlink: true,
	fontFamily: 'JetBrains Mono, ui-monospace, SFMono-Regular, Menlo, monospace',
	fontSize: 13,
	lineHeight: 1.0,
	scrollback: 5000,
	theme: terminalTheme
};
