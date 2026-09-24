import { describe, it, expect, beforeEach } from 'vitest';
import { clearInvokeMocks, invokeSpy, mockInvoke } from '../../../test/tauri-mocks';
import { PackageScripts, installCommand, runScriptCommand } from './package-scripts.svelte';
import type { PackageInfo } from '$types/workbench';

const info: PackageInfo = {
	manager: 'pnpm',
	managerVersion: '9.1.0',
	detectedFrom: 'packageManager',
	scripts: [{ name: 'test:unit', command: 'vitest' }]
};

describe('command builders', () => {
	it('uses the detected package manager', () => {
		expect(installCommand(info)).toBe('pnpm install');
		expect(runScriptCommand(info, info.scripts[0])).toBe('pnpm run test:unit');
	});
});

describe('PackageScripts', () => {
	beforeEach(() => clearInvokeMocks());

	it('stores the package info for a path', async () => {
		mockInvoke('get_package_info', () => info);
		const scripts = new PackageScripts();

		await scripts.load('/repo');

		expect(invokeSpy).toHaveBeenCalledWith('get_package_info', { path: '/repo' });
		expect(scripts.infoByPath['/repo']).toEqual(info);
	});

	it('stores null when there is no package.json', async () => {
		mockInvoke('get_package_info', () => null);
		const scripts = new PackageScripts();

		await scripts.load('/repo');

		expect(scripts.infoByPath['/repo']).toBeNull();
	});

	it('records an error and clears it after a successful reload', async () => {
		mockInvoke('get_package_info', () => {
			throw 'parsing /repo/package.json';
		});
		const scripts = new PackageScripts();

		await scripts.load('/repo');
		expect(scripts.errorByPath['/repo']).toBe('parsing /repo/package.json');

		mockInvoke('get_package_info', () => info);
		await scripts.load('/repo');
		expect(scripts.errorByPath['/repo']).toBeUndefined();
	});
});
