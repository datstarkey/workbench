import type { Terminal } from '@xterm/xterm';

const MAX_COMMANDS = 200;

export interface CommandEntry {
	promptStartLine: number;
	commandStartLine: number | null;
	outputStartLine: number | null;
	outputEndLine: number | null;
	exitCode: number | null;
	timestamp: number;
}

export class ShellIntegrationState {
	commands: CommandEntry[] = [];
	private _current: Partial<CommandEntry> | null = null;

	onPromptStart(line: number): void {
		if (this._current) {
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
			this._pushEntry(this._finalizeCurrent(line, exitCode));
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
		this.commands = [];
		this._current = null;
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
		while (this.commands.length > MAX_COMMANDS) {
			this.commands.shift();
		}
	}
}

export function registerShellIntegration(terminal: Terminal): ShellIntegrationState {
	const state = new ShellIntegrationState();

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

		return false;
	});

	return state;
}
