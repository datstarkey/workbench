import type { ChatImage } from '@workbench/types';
import { lsGet, lsRemove, lsSet } from './storage';
import type { ChatRef } from './types';

export const DRAFTS_KEY = 'wb.drafts';

export function draftKey(ref: ChatRef): string {
	return JSON.stringify([
		ref.agent ?? 'claude',
		ref.sessionId || ref.projectPath,
		ref.worktreePath ?? '',
		ref.claudeAccountId ?? ''
	]);
}

export class ChatDraft {
	text = $state('');
	images = $state<ChatImage[]>([]);
	constructor(text = '') {
		this.text = text;
	}
}

/** Text survives relaunch; attachments stay in memory while navigating between chats. */
export class Drafts {
	private drafts: Record<string, ChatDraft> = {};
	constructor(private readonly machineId: string) {}

	get(ref: ChatRef): ChatDraft {
		const key = draftKey(ref);
		return (this.drafts[key] ??= new ChatDraft(lsGet(this.storageKey(key)) ?? ''));
	}

	save(ref: ChatRef, draft: ChatDraft): void {
		const key = draftKey(ref);
		this.drafts[key] = draft;
		if (draft.text) lsSet(this.storageKey(key), draft.text);
		else lsRemove(this.storageKey(key));
	}

	/** Codex starts without an id; /clear also changes the key without losing the draft. */
	move(from: ChatRef, to: ChatRef, draft: ChatDraft): void {
		if (draftKey(from) === draftKey(to)) return;
		delete this.drafts[draftKey(from)];
		lsRemove(this.storageKey(draftKey(from)));
		this.save(to, draft);
	}

	private storageKey(key: string): string {
		return `${DRAFTS_KEY}.${this.machineId}.${key}`;
	}
}
