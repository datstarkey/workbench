import { uid } from '$lib/utils/uid';
import type {
	AccentColor,
	AgentAction,
	AgentActionTarget,
	ClaudePermissionMode,
	SessionType,
	SettingsWindowBounds,
	TerminalPerformanceMode,
	TerminalRenderer,
	WorkbenchSettings,
	WorktreeStartPoint,
	WorktreeStrategy
} from '$types/workbench';
import { invoke } from '$lib/transport';
// Desktop-local, like terminal IO: the srt settings file lives on this machine,
// so this one call must never be routed to a remote instance's control plane.
import { invoke as invokeLocal } from '@tauri-apps/api/core';
import { IS_WINDOWS, isClaudePermissionMode } from '$lib/utils/claude';
import { rotateServerToken } from '$lib/server-mode';

/** Fields on WorkbenchSettingsStore that can be updated via the generic `set()` method. */
type SettableField = keyof Omit<
	WorkbenchSettings,
	'agentActions' | 'claudeHooksApproved' | 'codexConfigApproved'
>;

export class WorkbenchSettingsStore {
	worktreeStrategy: WorktreeStrategy = $state('sibling');
	worktreeFetchBeforeCreate = $state(true);
	worktreeStartPoint: WorktreeStartPoint = $state('auto');
	worktreeCustomBranch = $state('');
	trelloEnabled = $state(false);
	gitSidebarEnabled = $state(false);
	terminalPerformanceMode: TerminalPerformanceMode = $state('auto');
	terminalTelemetryEnabled = $state(false);
	terminalRenderer: TerminalRenderer = $state<TerminalRenderer>('xterm');
	agentActions: AgentAction[] = $state([]);
	claudeHooksApproved: boolean | null = $state(null);
	codexConfigApproved: boolean | null = $state(null);
	claudePermissionMode: ClaudePermissionMode = $state<ClaudePermissionMode>('default');
	sandboxRuntimeEnabled = $state(false);
	sandboxAllowedDomains: string[] = $state([]);
	/**
	 * Absolute path of the generated srt settings file, resolved once from Rust.
	 * Empty when the backend could not provide it, in which case launches are not
	 * wrapped.
	 */
	sandboxRuntimeSettingsPath = $state('');
	cloneBaseDir: string | null = $state(null);
	accentColor: AccentColor = $state<AccentColor>('violet');
	serverMode = $state(false);
	serverPort = $state(4317);
	serverToken: string | null = $state(null);
	settingsWindowBounds: SettingsWindowBounds | null = $state(null);
	loaded = $state(false);
	saving = $state(false);
	dirty = $state(false);

	/**
	 * The srt settings file Claude launches should be wrapped with, or undefined
	 * to launch unwrapped. Native Windows support in sandbox-runtime is alpha, so
	 * the wrapper is Unix-only.
	 */
	get sandboxSettingsPath(): string | undefined {
		if (!this.sandboxRuntimeEnabled || IS_WINDOWS) return undefined;
		return this.sandboxRuntimeSettingsPath || undefined;
	}

	readonly runnableActions = $derived.by(() =>
		this.agentActions
			.map((a) => ({ ...a, name: a.name.trim(), prompt: a.prompt.trim() }))
			.filter((a) => a.name.length > 0 && a.prompt.length > 0)
	);

	async load() {
		const settings = await invoke<WorkbenchSettings>('load_workbench_settings');
		this.worktreeStrategy = settings.worktreeStrategy;
		this.worktreeFetchBeforeCreate = settings.worktreeFetchBeforeCreate ?? true;
		this.worktreeStartPoint = settings.worktreeStartPoint ?? 'auto';
		this.worktreeCustomBranch = settings.worktreeCustomBranch ?? '';
		this.trelloEnabled = settings.trelloEnabled;
		this.gitSidebarEnabled = settings.gitSidebarEnabled ?? false;
		this.terminalPerformanceMode = settings.terminalPerformanceMode ?? 'auto';
		this.terminalTelemetryEnabled = settings.terminalTelemetryEnabled ?? false;
		this.terminalRenderer = settings.terminalRenderer ?? 'xterm';
		this.agentActions = this.normalizeAgentActions(settings.agentActions);
		this.claudeHooksApproved = settings.claudeHooksApproved ?? null;
		this.codexConfigApproved = settings.codexConfigApproved ?? null;
		this.claudePermissionMode = isClaudePermissionMode(settings.claudePermissionMode)
			? settings.claudePermissionMode
			: 'default';
		this.sandboxRuntimeEnabled = settings.sandboxRuntimeEnabled ?? false;
		this.sandboxAllowedDomains = Array.isArray(settings.sandboxAllowedDomains)
			? settings.sandboxAllowedDomains
			: [];
		this.cloneBaseDir = settings.cloneBaseDir ?? null;
		this.accentColor = settings.accentColor ?? 'violet';
		this.serverMode = settings.serverMode ?? false;
		this.serverPort = settings.serverPort ?? 4317;
		this.serverToken = settings.serverToken ?? null;
		this.settingsWindowBounds = settings.settingsWindowBounds ?? null;
		this.loaded = true;
		this.dirty = false;

		// Resolved separately: the path is a backend-owned location, not a setting,
		// and the command writes the file before returning it. A failure here only
		// means launches go unwrapped, so it must not fail load().
		try {
			this.sandboxRuntimeSettingsPath = await invokeLocal<string>('sandbox_runtime_settings_path');
		} catch (e) {
			this.sandboxRuntimeSettingsPath = '';
			console.warn('[workbench-settings] sandbox_runtime_settings_path failed', e);
		}
	}

	async save() {
		this.saving = true;
		try {
			await invoke('save_workbench_settings', {
				settings: this.toSettings()
			});
			this.dirty = false;
		} finally {
			this.saving = false;
		}
	}

	set<K extends SettableField>(field: K, value: WorkbenchSettings[K]): void {
		// eslint-disable-next-line @typescript-eslint/no-explicit-any
		(this as any)[field] = value;
		this.dirty = true;
	}

	/** The LAN server token, creating one if none exists yet. */
	async ensureServerToken(): Promise<string> {
		return this.serverToken ?? this.rotateServerToken();
	}

	/**
	 * Replace the LAN server token. Rust persists only that field and restarts a
	 * running server, so unsaved edits elsewhere in the form (and `dirty`) are
	 * left alone rather than silently saved.
	 */
	async rotateServerToken(): Promise<string> {
		const token = await rotateServerToken();
		this.serverToken = token;
		return token;
	}

	addSandboxAllowedDomain(domain: string) {
		const value = domain.trim();
		if (!value || this.sandboxAllowedDomains.includes(value)) return;
		this.sandboxAllowedDomains = [...this.sandboxAllowedDomains, value];
		this.dirty = true;
	}

	removeSandboxAllowedDomain(domain: string) {
		this.sandboxAllowedDomains = this.sandboxAllowedDomains.filter((d) => d !== domain);
		this.dirty = true;
	}

	addAgentAction() {
		this.agentActions = [
			...this.agentActions,
			{
				id: uid(),
				name: '',
				prompt: '',
				target: 'both',
				category: '',
				tags: []
			}
		];
		this.dirty = true;
	}

	updateAgentAction(
		id: string,
		partial: Partial<Pick<AgentAction, 'name' | 'prompt' | 'target' | 'category' | 'tags'>>
	) {
		this.agentActions = this.agentActions.map((action) =>
			action.id === id ? { ...action, ...partial } : action
		);
		this.dirty = true;
	}

	removeAgentAction(id: string) {
		this.agentActions = this.agentActions.filter((action) => action.id !== id);
		this.dirty = true;
	}

	getApproval(type: SessionType): boolean | null {
		if (type === 'claude') return this.claudeHooksApproved;
		if (type === 'codex') return this.codexConfigApproved;
		return true;
	}

	async setApproval(type: SessionType, approved: boolean) {
		if (type === 'claude') this.claudeHooksApproved = approved;
		else if (type === 'codex') this.codexConfigApproved = approved;
		await invoke('save_workbench_settings', { settings: this.toSettings() });
	}

	private toSettings(): WorkbenchSettings {
		return {
			worktreeStrategy: this.worktreeStrategy,
			worktreeFetchBeforeCreate: this.worktreeFetchBeforeCreate,
			worktreeStartPoint: this.worktreeStartPoint,
			worktreeCustomBranch: this.worktreeCustomBranch,
			trelloEnabled: this.trelloEnabled,
			gitSidebarEnabled: this.gitSidebarEnabled,
			terminalPerformanceMode: this.terminalPerformanceMode,
			terminalTelemetryEnabled: this.terminalTelemetryEnabled,
			terminalRenderer: this.terminalRenderer,
			agentActions: this.agentActions,
			claudeHooksApproved: this.claudeHooksApproved,
			codexConfigApproved: this.codexConfigApproved,
			claudePermissionMode: this.claudePermissionMode,
			sandboxRuntimeEnabled: this.sandboxRuntimeEnabled,
			sandboxAllowedDomains: this.sandboxAllowedDomains,
			cloneBaseDir: this.cloneBaseDir,
			accentColor: this.accentColor,
			serverMode: this.serverMode,
			serverPort: this.serverPort,
			serverToken: this.serverToken,
			settingsWindowBounds: this.settingsWindowBounds
		};
	}

	/**
	 * Persist the settings window position/size. Quiet save (does not toggle
	 * `dirty`) — geometry isn't part of the form's save/discard flow.
	 */
	async setSettingsWindowBounds(bounds: SettingsWindowBounds) {
		this.settingsWindowBounds = bounds;
		await invoke('save_workbench_settings', { settings: this.toSettings() });
	}

	private normalizeAgentActions(actions: AgentAction[] | undefined): AgentAction[] {
		const safeActions = Array.isArray(actions) ? actions : [];
		return safeActions.map((action) => ({
			id: action.id || uid(),
			name: action.name ?? '',
			prompt: action.prompt ?? '',
			target: this.normalizeTarget(action.target),
			category: action.category ?? '',
			tags: this.normalizeTags(action.tags)
		}));
	}

	private normalizeTarget(target: string | undefined): AgentActionTarget {
		if (target === 'claude' || target === 'codex' || target === 'both') return target;
		return 'both';
	}

	private normalizeTags(tags: string[] | undefined): string[] {
		if (!Array.isArray(tags)) return [];
		return tags.map((tag) => tag.trim()).filter((tag) => tag.length > 0);
	}
}
