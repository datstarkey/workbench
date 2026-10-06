import { describe, it, expect } from 'vitest';
import {
	CODEX_NEW_SESSION_COMMAND,
	claudeSessionLaunch,
	codexCommand,
	codexCommandWithPrompt,
	codexResumeCommand,
	extractCodexPromptArg,
	tryCodexResumeCommand
} from './claude';

const validId = 'a1b2c3d4-e5f6-7890-abcd-ef1234567890';

describe('claudeSessionLaunch', () => {
	it("runs the pane's session, with an agent action's prompt", () => {
		const pane = { id: 'p', type: 'claude' as const, claudeSessionId: validId };
		expect(claudeSessionLaunch(pane)).toEqual({ id: validId });
		expect(claudeSessionLaunch({ ...pane, claudePrompt: 'Review' })).toEqual({
			id: validId,
			prompt: 'Review'
		});
	});

	it('is nothing for Codex, shell or id-less panes', () => {
		expect(claudeSessionLaunch({ id: 'p', type: 'codex', claudeSessionId: validId })).toBe(
			undefined
		);
		expect(claudeSessionLaunch({ id: 'p' })).toBe(undefined);
		expect(claudeSessionLaunch({ id: 'p', type: 'claude', claudeSessionId: '' })).toBe(undefined);
	});
});

describe('codex commands', () => {
	it('starts and resumes inline', () => {
		expect(CODEX_NEW_SESSION_COMMAND).toBe('codex');
		expect(codexCommand()).toBe('codex -c tui.alternate_screen=never');
		expect(codexResumeCommand(validId)).toBe(
			`codex -c tui.alternate_screen=never resume ${validId}`
		);
	});

	it('rejects non-UUID session IDs', () => {
		expect(() => codexResumeCommand('x; rm -rf ~')).toThrow('Invalid session ID');
		expect(tryCodexResumeCommand('x; rm -rf ~')).toBeUndefined();
	});

	it('submits a quoted first prompt, and none for a blank one', () => {
		expect(codexCommandWithPrompt('Find DRY violations')).toBe(
			"codex -c tui.alternate_screen=never 'Find DRY violations'"
		);
		expect(codexCommandWithPrompt('\n\t')).toBe('codex -c tui.alternate_screen=never');
		expect(codexCommandWithPrompt('line1\r\nline2')).toBe(
			"codex -c tui.alternate_screen=never 'line1\nline2'"
		);
		expect(codexCommandWithPrompt("it's broken")).toBe(
			"codex -c tui.alternate_screen=never 'it'\"'\"'s broken'"
		);
	});
});

describe('codex overrides', () => {
	const opts = {
		codexApprovalPolicy: 'never',
		codexSandboxMode: 'workspace-write'
	} as const;

	it('passes approval policy and sandbox mode as -c overrides', () => {
		expect(codexCommand(opts)).toBe(
			'codex -c tui.alternate_screen=never -c approval_policy=never -c sandbox_mode=workspace-write'
		);
		expect(codexResumeCommand(validId, opts)).toBe(
			`codex -c tui.alternate_screen=never -c approval_policy=never -c sandbox_mode=workspace-write resume ${validId}`
		);
	});

	it('writes nothing for default or unknown values', () => {
		expect(codexCommand({ codexApprovalPolicy: 'default', codexSandboxMode: 'default' })).toBe(
			'codex -c tui.alternate_screen=never'
		);
		expect(
			codexCommand({
				// Settings JSON is user-editable; nothing outside the allowlist reaches the shell.
				codexSandboxMode: 'read-only; rm -rf ~' as unknown as 'read-only'
			})
		).toBe('codex -c tui.alternate_screen=never');
	});

	it('strips the overrides when recovering a prompt', () => {
		const built = codexCommandWithPrompt('Find DRY violations', opts);
		expect(extractCodexPromptArg(built)).toBe("'Find DRY violations'");
	});
});

describe('extractCodexPromptArg', () => {
	it('is undefined for a bare binary, nothing, or another command', () => {
		expect(extractCodexPromptArg('codex')).toBeUndefined();
		expect(extractCodexPromptArg('codex -c tui.alternate_screen=never')).toBeUndefined();
		expect(extractCodexPromptArg(undefined)).toBeUndefined();
		expect(extractCodexPromptArg("claude 'hi'")).toBeUndefined();
	});

	it('returns the prompt argument', () => {
		expect(extractCodexPromptArg("codex 'Find DRY violations'")).toBe("'Find DRY violations'");
	});
});

describe('Codex terminal launch', () => {
	it('opts supported terminals out of the daemon and recovers persisted prompts', () => {
		const command = codexCommand({ codexNoDaemon: true });
		expect(command).toContain(' --no-daemon ');
		expect(extractCodexPromptArg(`${command} 'do work'`)).toBe("'do work'");
		expect(codexCommand()).not.toContain('--no-daemon');
	});
});
