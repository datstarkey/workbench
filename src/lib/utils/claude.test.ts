import { describe, it, expect } from 'vitest';
import {
	CLAUDE_NEW_SESSION_COMMAND,
	CODEX_NEW_SESSION_COMMAND,
	claudeResumeCommand,
	codexResumeCommand,
	newSessionCommandWithPrompt,
	newSessionCommand,
	resumeCommand
} from './claude';

describe('constants', () => {
	it('CLAUDE_NEW_SESSION_COMMAND equals "claude"', () => {
		expect(CLAUDE_NEW_SESSION_COMMAND).toBe('claude');
	});

	it('CODEX_NEW_SESSION_COMMAND equals "codex"', () => {
		expect(CODEX_NEW_SESSION_COMMAND).toBe('codex');
	});
});

describe('claudeResumeCommand', () => {
	it('returns correct command for valid UUID', () => {
		const id = 'a1b2c3d4-e5f6-7890-abcd-ef1234567890';
		expect(claudeResumeCommand(id)).toBe(`claude --resume ${id}`);
	});

	it('accepts uppercase UUID', () => {
		const id = 'A1B2C3D4-E5F6-7890-ABCD-EF1234567890';
		expect(claudeResumeCommand(id)).toBe(`claude --resume ${id}`);
	});

	it('throws for invalid UUID', () => {
		expect(() => claudeResumeCommand('not-a-uuid')).toThrow('Invalid session ID');
	});

	it('throws for empty string', () => {
		expect(() => claudeResumeCommand('')).toThrow('Invalid session ID');
	});

	it('throws for shell injection attempt with semicolons', () => {
		expect(() => claudeResumeCommand('a1b2c3d4-e5f6-7890-abcd-ef1234567890; rm -rf /')).toThrow(
			'Invalid session ID'
		);
	});

	it('throws for shell injection attempt with backticks', () => {
		expect(() => claudeResumeCommand('`malicious command`')).toThrow('Invalid session ID');
	});

	it('throws for UUID with extra characters', () => {
		expect(() => claudeResumeCommand('a1b2c3d4-e5f6-7890-abcd-ef1234567890-extra')).toThrow(
			'Invalid session ID'
		);
	});
});

describe('codexResumeCommand', () => {
	it('returns correct command for a session ID', () => {
		const id = 'a1b2c3d4-e5f6-7890-abcd-ef1234567890';
		expect(codexResumeCommand(id)).toBe(`codex resume ${id}`);
	});

	it('does not validate UUID format', () => {
		expect(codexResumeCommand('any-string')).toBe('codex resume any-string');
	});
});

describe('newSessionCommand', () => {
	it('returns codex command for "codex" type', () => {
		expect(newSessionCommand('codex')).toBe('codex');
	});

	it('returns claude command for "claude" type', () => {
		expect(newSessionCommand('claude')).toBe('claude');
	});

	it('returns claude command for "shell" type', () => {
		expect(newSessionCommand('shell')).toBe('claude');
	});
});

describe('resumeCommand', () => {
	const validId = 'a1b2c3d4-e5f6-7890-abcd-ef1234567890';

	it('dispatches to codexResumeCommand for "codex" type', () => {
		expect(resumeCommand('codex', validId)).toBe(`codex resume ${validId}`);
	});

	it('dispatches to claudeResumeCommand for "claude" type', () => {
		expect(resumeCommand('claude', validId)).toBe(`claude --resume ${validId}`);
	});

	it('dispatches to claudeResumeCommand for "shell" type', () => {
		expect(resumeCommand('shell', validId)).toBe(`claude --resume ${validId}`);
	});

	it('propagates validation errors from claudeResumeCommand', () => {
		expect(() => resumeCommand('claude', 'bad-id')).toThrow('Invalid session ID');
	});
});

describe('newSessionCommandWithPrompt', () => {
	it('returns base command when prompt is blank', () => {
		expect(newSessionCommandWithPrompt('claude', '   ')).toBe('claude');
		expect(newSessionCommandWithPrompt('codex', '\n\t')).toBe('codex');
	});

	it('adds a safely-quoted prompt for claude', () => {
		expect(newSessionCommandWithPrompt('claude', 'Review this PR for regressions')).toBe(
			"claude 'Review this PR for regressions'"
		);
	});

	it('adds a safely-quoted prompt for codex', () => {
		expect(newSessionCommandWithPrompt('codex', 'Find DRY violations')).toBe(
			"codex 'Find DRY violations'"
		);
	});

	it('escapes single quotes safely', () => {
		expect(newSessionCommandWithPrompt('claude', "it's broken")).toBe("claude 'it'\"'\"'s broken'");
	});

	it('normalizes windows newlines', () => {
		expect(newSessionCommandWithPrompt('codex', 'line1\r\nline2')).toBe("codex 'line1\nline2'");
	});
});
