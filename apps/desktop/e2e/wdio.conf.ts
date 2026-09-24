import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

// The repo itself is the seeded project: real git state and a real root package.json.
export const REPO_ROOT = resolve(import.meta.dirname, '../../..');

const binary = join(
	REPO_ROOT,
	'target/debug',
	process.platform === 'win32' ? 'workbench.exe' : 'workbench'
);

// Isolate ~/.workbench so a run never reads or writes the developer's real projects
// and settings. The launcher spawns the app with this process's env.
if (!process.env.WORKBENCH_CONFIG_DIR) {
	const configDir = mkdtempSync(join(tmpdir(), 'workbench-e2e-'));
	writeFileSync(
		join(configDir, 'projects.json'),
		JSON.stringify({ projects: [{ name: 'workbench', path: REPO_ROOT }] })
	);
	process.env.WORKBENCH_CONFIG_DIR = configDir;
}

// `tauri:options` has no type augmentation in @wdio/tauri-service; a named object
// skips the excess-property check on the capability literal.
const tauriCapability = { browserName: 'tauri', 'tauri:options': { application: binary } };

export const config: WebdriverIO.Config = {
	runner: 'local',
	specs: ['./specs/**/*.e2e.ts'],
	maxInstances: 1,
	capabilities: [tauriCapability],
	services: [['@wdio/tauri-service', { driverProvider: 'embedded' }]],
	framework: 'mocha',
	reporters: ['spec'],
	mochaOpts: { ui: 'bdd', timeout: 60_000 },
	waitforTimeout: 15_000,
	logLevel: 'warn',
	// The service auto-focuses windows before commands by reading window state through
	// `tauri-plugin-wdio`, which isn't installed here, so each attempt waits out a 5s
	// timeout. An explicit switch turns that off for the session; tests switch windows
	// themselves.
	before: async () => {
		await browser.switchToWindow(await browser.getWindowHandle());
	}
};
