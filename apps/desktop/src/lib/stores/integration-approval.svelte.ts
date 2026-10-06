import type { SessionType } from '$types/workbench';
import { applyCodexIntegration, checkCodexIntegration } from '$lib/utils/terminal';
import { getWorkbenchSettingsStore } from '$stores/context';

export class IntegrationApprovalStore {
	open = $state(false);
	description = $state('');
	error = $state('');

	private resolve: ((approved: boolean) => void) | null = null;
	private settings = getWorkbenchSettingsStore();

	async ensureIntegration(type: SessionType): Promise<boolean> {
		// Claude needs no settings changes: Workbench loads its own plugin into
		// every Claude process (`workbench_core::claude_plugin`).
		if (type !== 'codex') return true;

		const approval = this.settings.getApproval(type);

		if (approval === true) {
			try {
				await applyCodexIntegration();
			} catch {
				// Best-effort; don't block session creation
			}
			return true;
		}

		if (approval === false) {
			return true;
		}

		// Never asked (null) — check if changes are actually needed
		const status = await checkCodexIntegration();

		if (!status.needsChanges) {
			await this.settings.setApproval(type, true);
			return true;
		}

		// Show dialog and wait for user choice
		return this.showDialog(status.description);
	}

	async approve() {
		try {
			await applyCodexIntegration();
			await this.settings.setApproval('codex', true);
			this.error = '';
			this.open = false;
			this.resolve?.(true);
			this.resolve = null;
		} catch (e) {
			this.error = e instanceof Error ? e.message : String(e);
		}
	}

	skip() {
		this.settings.setApproval('codex', false);
		this.error = '';
		this.open = false;
		this.resolve?.(true);
		this.resolve = null;
	}

	/** Called when dialog is dismissed without a choice (X, Escape, outside click) */
	dismiss() {
		this.error = '';
		this.open = false;
		this.resolve?.(false);
		this.resolve = null;
	}

	private showDialog(description: string): Promise<boolean> {
		this.description = description;
		this.error = '';
		this.open = true;
		return new Promise<boolean>((resolve) => {
			this.resolve = resolve;
		});
	}
}
