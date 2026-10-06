/**
 * Cross-language integration: the real HttpTransport driving a real
 * workbench-server binary over HTTP. Skipped automatically unless the debug
 * binary has been built (`cargo build -p workbench-server`), so it never breaks
 * `turbo run test` in environments without the Rust toolchain.
 */
import { execFileSync, spawn, type ChildProcess } from 'node:child_process';
import { existsSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { createHttpTransport, type ControlPlaneTransport } from './index.ts';

const BIN = join(import.meta.dirname, '../../../target/debug/workbench-server');
const PORT = 47317;
const BASE = `http://127.0.0.1:${PORT}`;
const TOKEN = 'integration-token-0123456789abcdef01';

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
		projectDir = mkdtempSync(join(tmpdir(), 'wb-int-'));
		// A real repo, so the known-worktree guard runs rather than `git worktree list` failing.
		execFileSync('git', ['init', '-q'], { cwd: projectDir });

		// Register projectDir as a Workbench project so the cwd allowlist accepts
		// it (point the server's config dir at a throwaway projects.json).
		const configDir = mkdtempSync(join(tmpdir(), 'wb-int-cfg-'));
		writeFileSync(
			join(configDir, 'projects.json'),
			JSON.stringify({ projects: [{ name: 'int', path: projectDir }] })
		);

		server = spawn(BIN, ['--bind', '127.0.0.1', '--port', String(PORT)], {
			env: {
				...process.env,
				WORKBENCH_CONFIG_DIR: configDir,
				WORKBENCH_TOKEN: TOKEN
			},
			stdio: 'ignore'
		});
		await waitForHealth();
		transport = createHttpTransport({ baseUrl: BASE, token: TOKEN });
	}, 20000);

	afterAll(() => {
		server?.kill();
	});

	it('lists the registered projects through the transport', async () => {
		const projects = await transport.invoke('list_projects', undefined);
		expect(projects.map((p) => p.path)).toEqual([projectDir]);
	});

	it('throws a server error for an unknown worktree', async () => {
		await expect(
			transport.invoke('git_status', { path: '/nope/worktree', projectPath: projectDir })
		).rejects.toThrow(/not a known worktree/);
	});

	it('throws for commands the server does not expose', async () => {
		await expect(transport.invoke('save_projects', { projects: [] })).rejects.toThrow(
			/not supported/
		);
	});
});
