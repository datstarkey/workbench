export type SettingsScope = 'user' | 'user-local' | 'project' | 'project-local';

export type ScopeGroup = 'user' | 'project';

export interface SandboxNetworkConfig {
	allowedDomains?: string[];
	allowUnixSockets?: string[];
	allowAllUnixSockets?: boolean;
	allowLocalBinding?: boolean;
	httpProxyPort?: number;
	socksProxyPort?: number;
	[key: string]: unknown;
}

export interface SandboxConfig {
	enabled?: boolean;
	autoAllowBashIfSandboxed?: boolean;
	excludedCommands?: string[];
	allowUnsandboxedCommands?: boolean;
	enableWeakerNestedSandbox?: boolean;
	network?: SandboxNetworkConfig;
	[key: string]: unknown;
}

export interface PermissionsConfig {
	allow?: string[];
	deny?: string[];
	ask?: string[];
	additionalDirectories?: string[];
	defaultMode?: string;
	disableBypassPermissionsMode?: string;
	[key: string]: unknown;
}

export interface ClaudeSettings {
	// General
	preferredNotifChannel?: string;
	language?: string;
	outputStyle?: string;
	cleanupPeriodDays?: number;
	autoUpdatesChannel?: string;
	showTurnDuration?: boolean;
	spinnerTipsEnabled?: boolean;
	terminalProgressBarEnabled?: boolean;
	prefersReducedMotion?: boolean;
	respectGitignore?: boolean;

	// Thinking / effort
	alwaysThinkingEnabled?: boolean;
	effortLevel?: string;

	// Attribution
	attribution?: {
		commit?: string;
		pr?: string;
	};

	// Sandbox
	sandbox?: SandboxConfig;

	// Permissions
	permissions?: PermissionsConfig;

	// Plugins
	enabledPlugins?: Record<string, boolean> | string[];
	disabledPlugins?: string[];

	// MCP
	mcpServers?: Record<string, McpServerConfig>;
	enableAllProjectMcpServers?: boolean;
	enabledMcpjsonServers?: string[];
	disabledMcpjsonServers?: string[];

	// Hooks
	hooks?: Record<string, HookEntry[]>;
	disableAllHooks?: boolean;

	// Environment
	env?: Record<string, string>;

	// Catch-all for unknown keys
	[key: string]: unknown;
}

export interface McpServerConfig {
	command?: string;
	args?: string[];
	env?: Record<string, string>;
	disabled?: boolean;
	[key: string]: unknown;
}

export interface HookEntry {
	command: string;
	matcher?: string;
	timeout?: number;
	[key: string]: unknown;
}

export interface PluginInfo {
	name: string;
	description: string;
	version: string;
	dirName: string;
}

export interface SkillInfo {
	name: string;
	dirName: string;
	description: string;
}

export interface HookScriptInfo {
	name: string;
	path: string;
}
