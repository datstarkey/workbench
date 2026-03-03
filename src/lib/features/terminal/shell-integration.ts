import type { Terminal, IDecoration } from '@xterm/xterm';

const GUTTER_SUCCESS = '#4ade80';
const GUTTER_FAILURE = '#f87171';
const MAX_COMMANDS = 200;

export interface CommandEntry {
	promptStartLine: number;
	commandStartLine: number | null;
	outputStartLine: number | null;
	outputEndLine: number | null;
	exitCode: number | null;
	timestamp: number;
	decoration?: IDecoration;
}

export class ShellIntegrationState {
	commands: CommandEntry[] = [];
	private _current: Partial<CommandEntry> | null = null;
	private _terminal: Terminal | null = null;

	constructor(terminal: Terminal) {
		this._terminal = terminal;
	}

	onPromptStart(line: number): void {
		if (this._current) {
			// Finalize interrupted command (e.g., user hit Ctrl-C before executing)
			this._pushEntry(this._finalizeCurrent(null, null));
		}
		this._current = { promptStartLine: line, timestamp: Date.now() };
	}

	onCommandStart(line: number): void {
		if (this._current) {
			this._current.commandStartLine = line;
		}
	}

	onOutputStart(line: number): void {
		if (this._current) {
			this._current.outputStartLine = line;
		}
	}

	onCommandEnd(line: number, exitCode: number): void {
		if (this._current) {
			const entry = this._finalizeCurrent(line, exitCode);
			this._addDecoration(entry);
			this._pushEntry(entry);
			this._current = null;
		}
	}

	previousPromptLine(currentLine: number): number | null {
		for (let i = this.commands.length - 1; i >= 0; i--) {
			if (this.commands[i].promptStartLine < currentLine) {
				return this.commands[i].promptStartLine;
			}
		}
		return null;
	}

	nextPromptLine(currentLine: number): number | null {
		for (const cmd of this.commands) {
			if (cmd.promptStartLine > currentLine) {
				return cmd.promptStartLine;
			}
		}
		return null;
	}

	get latestCommand(): CommandEntry | null {
		return this.commands.length > 0 ? this.commands[this.commands.length - 1] : null;
	}

	dispose(): void {
		for (const cmd of this.commands) {
			cmd.decoration?.dispose();
		}
		this.commands = [];
		this._current = null;
		this._terminal = null;
	}

	private _finalizeCurrent(outputEndLine: number | null, exitCode: number | null): CommandEntry {
		const c = this._current!;
		return {
			promptStartLine: c.promptStartLine ?? 0,
			commandStartLine: c.commandStartLine ?? null,
			outputStartLine: c.outputStartLine ?? null,
			outputEndLine,
			exitCode,
			timestamp: c.timestamp ?? Date.now()
		};
	}

	private _pushEntry(entry: CommandEntry): void {
		this.commands.push(entry);
		// Evict oldest entries to bound memory
		while (this.commands.length > MAX_COMMANDS) {
			const old = this.commands.shift();
			old?.decoration?.dispose();
		}
	}

	private _addDecoration(entry: CommandEntry): void {
		if (!this._terminal) return;
		const buf = this._terminal.buffer.active;
		const rowOffset = entry.promptStartLine - buf.baseY;
		const marker = this._terminal.registerMarker(rowOffset - buf.cursorY);
		if (!marker) return;

		const color = entry.exitCode === 0 ? GUTTER_SUCCESS : GUTTER_FAILURE;
		const decoration = this._terminal.registerDecoration({
			marker,
			anchor: 'left',
			overviewRulerOptions: { color, position: 'left' }
		});

		decoration?.onRender((element) => {
			element.style.width = '3px';
			element.style.height = '100%';
			element.style.backgroundColor = color;
			element.style.borderRadius = '1px';
		});

		entry.decoration = decoration ?? undefined;
	}
}

export function registerShellIntegration(terminal: Terminal): ShellIntegrationState {
	const state = new ShellIntegrationState(terminal);

	terminal.parser.registerOscHandler(133, (data: string) => {
		const buf = terminal.buffer.active;
		const currentLine = buf.cursorY + buf.baseY;

		if (data === 'A') {
			state.onPromptStart(currentLine);
		} else if (data === 'B') {
			state.onCommandStart(currentLine);
		} else if (data === 'C') {
			state.onOutputStart(currentLine);
		} else if (data.startsWith('D')) {
			const parts = data.split(';');
			const exitCode = parts.length > 1 ? parseInt(parts[1], 10) : 0;
			state.onCommandEnd(currentLine, isNaN(exitCode) ? 0 : exitCode);
		}

		return false; // don't consume — let other handlers see the sequence
	});

	return state;
}
