import { describe, it, expect } from 'vitest';
import {
	CLAUDE_NEW_SESSION_COMMAND,
	CODEX_NEW_SESSION_COMMAND,
	claudeResumeCommand,
	codexResumeCommand,
	extractPromptArg,
	newSessionCommandWithPrompt,
	newSessionCommand,
	resumeCommand
} from './claude';
import type { ClaudePermissionMode } from '$types/workbench';

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

describe('permission mode', () => {
	const validId = 'a1b2c3d4-e5f6-7890-abcd-ef1234567890';

	it('appends --permission-mode to a new session command', () => {
		expect(newSessionCommand('claude', { permissionMode: 'bypassPermissions' })).toBe(
			'claude --permission-mode bypassPermissions'
		);
	});

	it('appends --permission-mode before --resume', () => {
		expect(resumeCommand('claude', validId, { permissionMode: 'bypassPermissions' })).toBe(
			`claude --permission-mode bypassPermissions --resume ${validId}`
		);
	});

	it('appends --permission-mode before an initial prompt', () => {
		expect(
			newSessionCommandWithPrompt('claude', 'Fix the build', {
				permissionMode: 'bypassPermissions'
			})
		).toBe("claude --permission-mode bypassPermissions 'Fix the build'");
	});

	it('omits the flag for the "default" mode', () => {
		expect(newSessionCommand('claude', { permissionMode: 'default' })).toBe('claude');
		expect(resumeCommand('claude', validId, { permissionMode: 'default' })).toBe(
			`claude --resume ${validId}`
		);
	});

	it('omits the flag for a mode outside the allowlist', () => {
		const rogue = 'plan; rm -rf /' as ClaudePermissionMode;
		expect(newSessionCommand('claude', { permissionMode: rogue })).toBe('claude');
		expect(resumeCommand('claude', validId, { permissionMode: rogue })).toBe(
			`claude --resume ${validId}`
		);
	});

	it('accepts every documented mode', () => {
		for (const mode of ['acceptEdits', 'plan', 'dontAsk', 'auto'] as const) {
			expect(newSessionCommand('claude', { permissionMode: mode })).toBe(
				`claude --permission-mode ${mode}`
			);
		}
	});

	it('always builds on the claude binary', () => {
		expect(newSessionCommand('claude', { permissionMode: 'acceptEdits' })).toBe(
			'claude --permission-mode acceptEdits'
		);
		expect(claudeResumeCommand(validId, { permissionMode: 'acceptEdits' })).toBe(
			`claude --permission-mode acceptEdits --resume ${validId}`
		);
	});

	it('leaves codex commands untouched', () => {
		expect(newSessionCommand('codex', { permissionMode: 'bypassPermissions' })).toBe('codex');
		expect(resumeCommand('codex', validId, { permissionMode: 'bypassPermissions' })).toBe(
			`codex resume ${validId}`
		);
		expect(
			newSessionCommandWithPrompt('codex', 'Find DRY violations', {
				permissionMode: 'bypassPermissions'
			})
		).toBe("codex 'Find DRY violations'");
	});
});

describe('extractPromptArg', () => {
	it('returns undefined for a bare binary', () => {
		expect(extractPromptArg('claude', 'claude')).toBeUndefined();
		expect(extractPromptArg('codex', 'codex')).toBeUndefined();
	});

	it('returns undefined for a missing or blank command', () => {
		expect(extractPromptArg('claude', undefined)).toBeUndefined();
		expect(extractPromptArg('claude', '   ')).toBeUndefined();
	});

	it('returns undefined for a command built on a different binary', () => {
		expect(extractPromptArg('claude', "codex 'hi'")).toBeUndefined();
		expect(extractPromptArg('claude', 'npm run dev')).toBeUndefined();
	});

	it('returns the prompt argument with no flag present', () => {
		expect(extractPromptArg('claude', "claude 'Review this PR'")).toBe("'Review this PR'");
		expect(extractPromptArg('codex', "codex 'Find DRY violations'")).toBe("'Find DRY violations'");
	});

	it('strips a permission-mode flag before the prompt', () => {
		expect(extractPromptArg('claude', "claude --permission-mode bypassPermissions 'Review'")).toBe(
			"'Review'"
		);
		expect(extractPromptArg('claude', "claude --permission-mode plan 'Review'")).toBe("'Review'");
	});

	it('returns undefined when only a permission-mode flag follows the binary', () => {
		expect(extractPromptArg('claude', 'claude --permission-mode acceptEdits')).toBeUndefined();
	});

	it('returns undefined for an unknown permission mode', () => {
		expect(extractPromptArg('claude', "claude --permission-mode bogus 'Review'")).toBeUndefined();
		expect(
			extractPromptArg('claude', "claude --permission-mode plan; rm -rf / 'Review'")
		).toBeUndefined();
	});

	it('does not strip the flag for codex commands', () => {
		expect(extractPromptArg('codex', "codex --permission-mode plan 'Review'")).toBe(
			"--permission-mode plan 'Review'"
		);
	});

	it('round-trips a command built by newSessionCommandWithPrompt', () => {
		const opts = { permissionMode: 'bypassPermissions' } as const;
		const built = newSessionCommandWithPrompt('claude', "it's broken", opts);
		const promptArg = extractPromptArg('claude', built);
		expect(promptArg).toBeDefined();
		expect(`${newSessionCommand('claude', opts)} ${promptArg}`).toBe(built);
	});
});
