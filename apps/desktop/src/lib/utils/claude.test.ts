import { describe, it, expect, vi } from 'vitest';
import {
	CLAUDE_NEW_SESSION_COMMAND,
	CODEX_NEW_SESSION_COMMAND,
	claudeResumeCommand,
	codexResumeCommand,
	extractPromptArg,
	newSessionCommandWithPrompt,
	newSessionCommand,
	resumeCommand,
	applyClaudeLaunchOptions,
	SANDBOX_RUNTIME_PACKAGE
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

describe('sandbox runtime wrapper', () => {
	const sessionId = '12345678-1234-1234-1234-123456789abc';
	const settingsPath = '/Users/u/.workbench/sandbox-runtime.json';
	const opts = { sandboxSettingsPath: settingsPath } as const;
	const prefix = `npx --yes ${SANDBOX_RUNTIME_PACKAGE} --settings '${settingsPath}' --`;

	it('wraps a new-session command', () => {
		expect(newSessionCommand('claude', opts)).toBe(`${prefix} claude`);
	});

	it('wraps a resume command', () => {
		expect(claudeResumeCommand(sessionId, opts)).toBe(`${prefix} claude --resume ${sessionId}`);
	});

	/**
	 * srt parses its own options anywhere in the argument list, so the separator
	 * is what stops it eating `--permission-mode`'s neighbours or a `--resume`.
	 */
	it('puts the -- separator between srt and the wrapped binary', () => {
		expect(newSessionCommand('claude', opts)).toContain(' -- claude');
	});

	it('orders the wrapper before the permission-mode flag', () => {
		expect(newSessionCommand('claude', { ...opts, permissionMode: 'bypassPermissions' })).toBe(
			`${prefix} claude --permission-mode bypassPermissions`
		);
	});

	it('never wraps codex', () => {
		expect(newSessionCommand('codex', opts)).toBe(CODEX_NEW_SESSION_COMMAND);
		expect(resumeCommand('codex', sessionId, opts)).toBe(`codex resume ${sessionId}`);
	});

	it('does not wrap when no settings path is configured', () => {
		expect(newSessionCommand('claude', { sandboxSettingsPath: '' })).toBe('claude');
		expect(newSessionCommand('claude', {})).toBe('claude');
	});

	it('shell-quotes a settings path containing a space', () => {
		const spaced = '/Users/u/My Files/sandbox-runtime.json';
		expect(newSessionCommand('claude', { sandboxSettingsPath: spaced })).toBe(
			`npx --yes ${SANDBOX_RUNTIME_PACKAGE} --settings '${spaced}' -- claude`
		);
	});

	it('recovers the prompt from a wrapped command', () => {
		expect(extractPromptArg('claude', `${prefix} claude 'Review this PR'`)).toBe(
			"'Review this PR'"
		);
	});

	it('recovers the prompt from a wrapped command with a quoted, spaced path', () => {
		const spaced = '/Users/u/My Files/sandbox-runtime.json';
		const built = newSessionCommandWithPrompt('claude', 'Review this PR', {
			sandboxSettingsPath: spaced
		});
		expect(extractPromptArg('claude', built)).toBe("'Review this PR'");
	});

	it('round-trips a wrapped command built with a prompt and a mode', () => {
		const full = { ...opts, permissionMode: 'bypassPermissions' } as const;
		const built = newSessionCommandWithPrompt('claude', "it's broken", full);
		const promptArg = extractPromptArg('claude', built);
		expect(promptArg).toBeDefined();
		expect(`${newSessionCommand('claude', full)} ${promptArg}`).toBe(built);
	});

	it('leaves an unwrapped command alone when recovering the prompt', () => {
		expect(extractPromptArg('claude', "claude 'Review this PR'")).toBe("'Review this PR'");
	});

	it('yields no prompt for a wrapped command that has none', () => {
		expect(extractPromptArg('claude', `${prefix} claude`)).toBeUndefined();
	});
});

describe('sandbox runtime wrapper hardening', () => {
	const settingsPath = '/Users/u/.workbench/sandbox-runtime.json';
	const opts = { sandboxSettingsPath: settingsPath } as const;
	const prefix = `npx --yes ${SANDBOX_RUNTIME_PACKAGE} --settings '${settingsPath}' --`;

	/** An upstream release must not be able to change sandbox semantics silently. */
	it('pins the wrapper to an exact version', () => {
		expect(SANDBOX_RUNTIME_PACKAGE).toBe('@anthropic-ai/sandbox-runtime@0.0.76');
		expect(newSessionCommand('claude', opts)).toContain('@anthropic-ai/sandbox-runtime@0.0.76');
	});

	/** Commands persisted before the pin landed carry no version. */
	it('strips an unversioned wrapper written by an earlier build', () => {
		const legacy =
			"npx --yes @anthropic-ai/sandbox-runtime --settings '/old/path.json' -- claude 'Review'";
		expect(extractPromptArg('claude', legacy)).toBe("'Review'");
	});

	it('strips a wrapper pinned to a different version', () => {
		const other =
			"npx --yes @anthropic-ai/sandbox-runtime@9.9.9 --settings '/old/path.json' -- claude 'Review'";
		expect(extractPromptArg('claude', other)).toBe("'Review'");
	});

	/** srt's native Windows support is alpha, so the builder refuses to wrap there. */
	it('does not wrap on Windows even when a settings path is given', async () => {
		vi.resetModules();
		vi.stubGlobal('navigator', {
			userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)'
		});
		try {
			const win = await import('./claude');
			expect(win.newSessionCommand('claude', { sandboxSettingsPath: 'C:\\wb\\srt.json' })).toBe(
				'claude'
			);
			expect(
				win.claudeResumeCommand('12345678-1234-1234-1234-123456789abc', {
					sandboxSettingsPath: 'C:\\wb\\srt.json',
					permissionMode: 'bypassPermissions'
				})
			).toBe(
				'claude --permission-mode bypassPermissions --resume 12345678-1234-1234-1234-123456789abc'
			);
		} finally {
			vi.unstubAllGlobals();
			vi.resetModules();
		}
	});

	describe('applyClaudeLaunchOptions', () => {
		it('wraps a bare claude command', () => {
			expect(applyClaudeLaunchOptions('claude', opts)).toBe(`${prefix} claude`);
		});

		it('keeps the arguments of an explicit claude command', () => {
			expect(applyClaudeLaunchOptions("claude 'run the tests'", opts)).toBe(
				`${prefix} claude 'run the tests'`
			);
		});

		it('adds a permission-mode flag', () => {
			expect(applyClaudeLaunchOptions('claude', { permissionMode: 'plan' })).toBe(
				'claude --permission-mode plan'
			);
		});

		/**
		 * A permission flag the user wrote themselves is their decision: apply the
		 * sandbox wrapper, but do not override the posture they chose.
		 */
		it("preserves a user's own permission-mode flag instead of overriding it", () => {
			expect(
				applyClaudeLaunchOptions('claude --permission-mode plan', {
					permissionMode: 'acceptEdits'
				})
			).toBe('claude --permission-mode plan');
		});

		it("wraps but does not re-flag a command carrying the user's own mode", () => {
			expect(
				applyClaudeLaunchOptions('claude --permission-mode plan', {
					...opts,
					permissionMode: 'acceptEdits'
				})
			).toBe(`${prefix} claude --permission-mode plan`);
		});

		it('leaves --dangerously-skip-permissions alone', () => {
			expect(
				applyClaudeLaunchOptions('claude --dangerously-skip-permissions', {
					permissionMode: 'plan'
				})
			).toBe('claude --dangerously-skip-permissions');
			expect(
				applyClaudeLaunchOptions('claude --allow-dangerously-skip-permissions', {
					...opts,
					permissionMode: 'plan'
				})
			).toBe(`${prefix} claude --allow-dangerously-skip-permissions`);
		});

		/** Never silently discard what the user wrote, even when unparseable. */
		it('keeps an unparseable remainder verbatim rather than dropping it', () => {
			expect(applyClaudeLaunchOptions("claude --permission-mode bogus 'Review'", opts)).toBe(
				`${prefix} claude --permission-mode bogus 'Review'`
			);
		});

		it('does not treat a bare word containing a flag name as a permission flag', () => {
			expect(
				applyClaudeLaunchOptions("claude 'explain --permission-mode'", {
					permissionMode: 'plan'
				})
			).toBe("claude --permission-mode plan 'explain --permission-mode'");
		});

		it('rewraps an already-wrapped command rather than nesting wrappers', () => {
			const once = applyClaudeLaunchOptions('claude', opts);
			expect(applyClaudeLaunchOptions(once, opts)).toBe(once);
			expect(applyClaudeLaunchOptions(once, opts).match(/npx/g)).toHaveLength(1);
		});

		it('unwraps when no settings path is supplied', () => {
			expect(applyClaudeLaunchOptions(`${prefix} claude 'x'`, {})).toBe("claude 'x'");
		});

		it('leaves a non-claude command untouched', () => {
			expect(applyClaudeLaunchOptions('bun run dev', opts)).toBe('bun run dev');
			expect(applyClaudeLaunchOptions('echo claude', opts)).toBe('echo claude');
			expect(applyClaudeLaunchOptions('claudefoo', opts)).toBe('claudefoo');
		});

		it('preserves a resume flag', () => {
			const id = '12345678-1234-1234-1234-123456789abc';
			expect(applyClaudeLaunchOptions(`claude --resume ${id}`, opts)).toBe(
				`${prefix} claude --resume ${id}`
			);
		});
	});
});
