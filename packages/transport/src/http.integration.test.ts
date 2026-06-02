/**
 * Cross-language integration: the real HttpTransport driving a real
 * workbench-server binary over HTTP. Skipped automatically unless the debug
 * binary has been built (`cargo build -p workbench-server`), so it never breaks
 * `turbo run test` in environments without the Rust toolchain.
 */
import { spawn, type ChildProcess } from 'node:child_process';
import { chmodSync, existsSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { createHttpTransport, type ControlPlaneTransport } from './index.ts';

const BIN = join(import.meta.dirname, '../../../target/debug/workbench-server');
const PORT = 47317;
const BASE = `http://127.0.0.1:${PORT}`;

const hasBin = existsSync(BIN);

async function waitForHealth(timeoutMs = 5000) {
	const deadline = Date.now() + timeoutMs;
	while (Date.now() < deadline) {
		try {
			const res = await fetch(`${BASE}/health`);
			if (res.ok) return;
		} catch {
			/* not up yet */
		}
		await new Promise((r) => setTimeout(r, 100));
	}
	throw new Error('server did not become healthy');
}

describe.skipIf(!hasBin)('HttpTransport ↔ real workbench-server', () => {
	let server: ChildProcess;
	let projectDir: string;
	let transport: ControlPlaneTransport;

	beforeAll(async () => {
		const dir = mkdtempSync(join(tmpdir(), 'wb-int-'));
		projectDir = dir;
		// Fake claude: print a session URL, then stay alive.
		const fake = join(dir, 'fake-claude.sh');
		writeFileSync(fake, '#!/bin/sh\necho "Session: https://claude.ai/code/int-test"\nsleep 30\n');
		chmodSync(fake, 0o755);

		server = spawn(BIN, ['--port', String(PORT)], {
			env: { ...process.env, WORKBENCH_CLAUDE_BIN: fake },
			stdio: 'ignore'
		});
		await waitForHealth();
		transport = createHttpTransport({ baseUrl: BASE });
	}, 20000);

	afterAll(() => {
		server?.kill();
	});

	it('spawns, lists, and kills a session through the transport', async () => {
		const spawned = (await transport.invoke('remote_spawn', {
			projectPath: projectDir,
			name: 'int'
		})) as { id: string; name: string };
		expect(spawned.id).toBeTruthy();
		expect(spawned.name).toBe('int');

		// Poll until the reader thread captures the URL.
		let url: string | null = null;
		for (let i = 0; i < 40 && !url; i++) {
			await new Promise((r) => setTimeout(r, 100));
			const sessions = (await transport.invoke('remote_sessions', undefined)) as Array<{
				sessionUrl: string | null;
			}>;
			url = sessions[0]?.sessionUrl ?? null;
		}
		expect(url).toBe('https://claude.ai/code/int-test');

		await transport.invoke('remote_kill', { id: spawned.id });
		const after = (await transport.invoke('remote_sessions', undefined)) as unknown[];
		expect(after).toHaveLength(0);
	}, 15000);

	it('throws a server error for an unknown worktree', async () => {
		await expect(
			transport.invoke('remote_spawn', {
				projectPath: projectDir,
				worktreePath: '/nope/worktree'
			})
		).rejects.toThrow();
	});

	it('throws for commands the server does not expose', async () => {
		await expect(transport.invoke('save_projects', { projects: [] })).rejects.toThrow(
			/not supported/
		);
	});
});
