import type { CodexAction } from '@workbench/types';
import type { AgentChat } from './agent-chat.svelte';
import { decodeAudio, pcm16 } from './codex-helpers';

export class CodexControlsStore {
	constructor(
		readonly chat: AgentChat,
		readonly onThread: (id: string, label: string) => void
	) {}
	tab = $state('actions');
	busy = $state(false);
	error = $state('');
	result = $state<unknown>(null);
	title = $state('');
	reviewType = $state('uncommittedChanges');
	reviewValue = $state('');
	objective = $state('');
	budget = $state('');
	search = $state('');
	archived = $state(false);
	cursor = $state<string | null>(null);
	threads = $state<{ id: string; name: string | null; preview: string; updatedAt: number }[]>([]);
	section = $state('account');
	mcpName = $state('');
	processId = $state('');
	loginId = $state('');
	environmentId = $state('');
	clientId = $state('');
	editing = $state<Record<string, string>>({});
	mic = $state(false);
	context: AudioContext | null = null;
	stream: MediaStream | null = null;
	processor: ScriptProcessorNode | null = null;
	outputAt = 0;
	audioPending = 0;
	ownsVoice = false;
	private disposed = false;
	private startingVoice = false;

	async run(action: CodexAction, params: Record<string, unknown> = {}): Promise<unknown> {
		this.busy = true;
		this.error = '';
		try {
			this.result = await this.chat.codexAction(action, params);
			const environment = (this.result as { environmentId?: string } | null)?.environmentId;
			if (environment) this.environmentId = environment;
			return this.result;
		} catch (e) {
			this.error = e instanceof Error ? e.message : String(e);
			return null;
		} finally {
			this.busy = false;
		}
	}
	async fork() {
		const v = (await this.run('fork')) as { thread?: { id: string; name?: string | null } } | null;
		if (v?.thread?.id) this.onThread(v.thread.id, v.thread.name ?? 'Forked conversation');
	}
	async list(more = false) {
		const v = (await this.run('threads', {
			search: this.search,
			archived: this.archived,
			...(more ? { cursor: this.cursor } : {})
		})) as {
			data: { id: string; name: string | null; preview: string; updatedAt: number }[];
			nextCursor: string | null;
		} | null;
		if (v) {
			this.threads = more ? [...this.threads, ...v.data] : v.data;
			this.cursor = v.nextCursor;
		}
	}
	async attachFiles(event: Event) {
		const input = event.currentTarget as HTMLInputElement;
		for (const file of Array.from(input.files ?? [])) {
			if (file.size > 64 * 1024) {
				this.error = `${file.name} is too large. Choose a text file under 64 KB.`;
				continue;
			}
			const text = await file.text();
			if (text.includes('\0')) {
				this.error = `${file.name} is not a text file.`;
				continue;
			}
			if (this.chat.files.length >= 10) {
				this.error = 'Attach at most ten files.';
				break;
			}
			this.chat.files = [...this.chat.files, { name: file.name, text }];
		}
		input.value = '';
	}
	async startVoice() {
		if (this.disposed || this.startingVoice || this.mic) return;
		this.error = '';
		if (!navigator.mediaDevices?.getUserMedia) {
			this.error = 'Microphone access is unavailable on this device.';
			return;
		}
		this.startingVoice = true;
		try {
			this.stream = await navigator.mediaDevices.getUserMedia({
				audio: { channelCount: 1, echoCancellation: true }
			});
			if (this.disposed) {
				await this.stopVoice();
				return;
			}
			this.context = new AudioContext({ sampleRate: 24000 });
			await this.context.resume();
			if (this.disposed) {
				await this.stopVoice();
				return;
			}
			await this.chat.codexAction('realtimeStart');
			this.ownsVoice = true;
			if (this.disposed) {
				await this.stopVoice();
				return;
			}
			const source = this.context.createMediaStreamSource(this.stream);
			this.processor = this.context.createScriptProcessor(2048, 1, 1);
			this.processor.onaudioprocess = (e) => {
				if (this.audioPending >= 2) return;
				this.audioPending++;
				void this.chat
					.codexAction('realtimeAudio', {
						data: pcm16(e.inputBuffer.getChannelData(0)),
						sampleRate: this.context?.sampleRate ?? 24000
					})
					.catch((e) => {
						this.error = String(e);
						void this.stopVoice();
					})
					.finally(() => this.audioPending--);
			};
			source.connect(this.processor);
			this.processor.connect(this.context.destination);
			this.mic = true;
			this.chat.onCodexEvent = (method, p) => {
				if (method === 'thread/realtime/outputAudio/delta' && this.context) {
					const audio = p.audio as { data: string; sampleRate: number; numChannels: number };
					try {
						const samples = decodeAudio(audio.data);
						const channels = audio.numChannels || 1;
						const buffer = this.context.createBuffer(
							channels,
							samples.length / channels,
							audio.sampleRate
						);
						for (let c = 0; c < channels; c++)
							buffer.copyToChannel(
								Float32Array.from({ length: buffer.length }, (_, i) => samples[i * channels + c]),
								c
							);
						const source = this.context.createBufferSource();
						source.buffer = buffer;
						source.connect(this.context.destination);
						this.outputAt = Math.max(this.context.currentTime, this.outputAt);
						source.start(this.outputAt);
						this.outputAt += buffer.duration;
					} catch (e) {
						this.error = String(e);
					}
				}
				if (method === 'thread/realtime/closed' || method === 'thread/realtime/error')
					void this.stopVoice();
			};
		} catch (e) {
			this.error = e instanceof Error ? e.message : String(e);
			await this.stopVoice();
		} finally {
			this.startingVoice = false;
		}
	}
	async stopVoice() {
		this.processor?.disconnect();
		this.processor = null;
		this.stream?.getTracks().forEach((t) => t.stop());
		this.stream = null;
		void this.context?.close();
		this.context = null;
		this.mic = false;
		this.chat.onCodexEvent = null;
		this.outputAt = 0;
		if (this.ownsVoice) {
			this.ownsVoice = false;
			await this.chat.codexAction('realtimeStop').catch(() => {});
		}
	}
	dispose(): void {
		this.disposed = true;
		void this.stopVoice();
	}
}
