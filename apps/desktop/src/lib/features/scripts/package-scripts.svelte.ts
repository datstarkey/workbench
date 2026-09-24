import { invoke } from '@tauri-apps/api/core';
import type { PackageInfo, PackageScript } from '$types/workbench';

export function installCommand(info: PackageInfo): string {
	return `${info.manager} install`;
}

/** Script names are pre-filtered in Rust to characters no shell treats specially. */
export function runScriptCommand(info: PackageInfo, script: PackageScript): string {
	return `${info.manager} run ${script.name}`;
}

/** Root `package.json` per workspace path, read on demand. `undefined` = not loaded yet. */
export class PackageScripts {
	infoByPath = $state<Record<string, PackageInfo | null>>({});
	errorByPath = $state<Record<string, string>>({});

	async load(path: string): Promise<void> {
		try {
			this.infoByPath[path] = await invoke<PackageInfo | null>('get_package_info', { path });
			delete this.errorByPath[path];
		} catch (error) {
			this.errorByPath[path] = String(error);
		}
	}
}
