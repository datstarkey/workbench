import type { SessionType } from '$types/workbench';
import {
	applyClaudeIntegration,
	applyCodexIntegration,
	checkClaudeIntegration,
	checkCodexIntegration
} from '$lib/utils/terminal';
import { getWorkbenchSettingsStore } from '$stores/context';

export class IntegrationApprovalStore {
	open = $state(false);
	description = $state('');
	sessionType: SessionType = $state('claude');
	error = $state('');

	private resolve: ((approved: boolean) => void) | null = null;
	private settings = getWorkbenchSettingsStore();

	async ensureIntegration(type: SessionType): Promise<boolean> {
		if (type === 'shell') return true;

		const approval = this.settings.getApproval(type);

		if (approval === true) {
			try {
				await this.applyForType(type);
			} catch {
				// Best-effort; don't block session creation
			}
			return true;
		}

		if (approval === false) {
			return true;
		}

		// Never asked (null) — check if changes are actually needed
		const status =
			type === 'claude' ? await checkClaudeIntegration() : await checkCodexIntegration();

		if (!status.needsChanges) {
			await this.settings.setApproval(type, true);
			return true;
		}

		// Show dialog and wait for user choice
		return this.showDialog(type, status.description);
	}

	async approve() {
		try {
			await this.applyForType(this.sessionType);
			await this.settings.setApproval(this.sessionType, true);
			this.error = '';
			this.open = false;
			this.resolve?.(true);
			this.resolve = null;
		} catch (e) {
			this.error = e instanceof Error ? e.message : String(e);
		}
	}

	skip() {
		this.settings.setApproval(this.sessionType, false);
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

	private showDialog(type: SessionType, description: string): Promise<boolean> {
		this.sessionType = type;
		this.description = description;
		this.error = '';
		this.open = true;
		return new Promise<boolean>((resolve) => {
			this.resolve = resolve;
		});
	}

	private async applyForType(type: SessionType): Promise<void> {
		if (type === 'claude') await applyClaudeIntegration();
		else if (type === 'codex') await applyCodexIntegration();
	}
}
