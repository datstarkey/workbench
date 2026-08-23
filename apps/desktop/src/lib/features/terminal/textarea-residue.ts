/**
 * Keeps xterm's hidden input `<textarea>` empty between keystrokes.
 *
 * xterm only clears that textarea on Enter / Ctrl+C and never cancels
 * `keypress`, so every key it handles there — on WebKit that's A–Z and space —
 * is also inserted into the textarea and accumulates ("H W THIS I A T ").
 * WKWebView emits stray `compositionstart`s (Caps Lock, press-and-hold,
 * input-source switches); the next keydown makes xterm finalise that
 * "composition" with stale positions and emit the entire residue as typed
 * input — the repeated-capitals-plus-extra-spaces bug. With nothing in the
 * textarea there is nothing to flush.
 *
 * Clearing is deferred a macrotask and skipped while a real IME composition is
 * in progress, so xterm's own deferred reads of the textarea still see the
 * composed text.
 */
export function installTextareaResidueGuard(textarea: HTMLTextAreaElement): () => void {
	let composing = false;
	const onCompositionStart = () => {
		composing = true;
	};
	const onCompositionEnd = () => {
		composing = false;
	};
	const onInput = () => {
		if (composing) return;
		setTimeout(() => {
			if (!composing) textarea.value = '';
		}, 0);
	};
	textarea.addEventListener('compositionstart', onCompositionStart);
	textarea.addEventListener('compositionend', onCompositionEnd);
	textarea.addEventListener('input', onInput);
	return () => {
		textarea.removeEventListener('compositionstart', onCompositionStart);
		textarea.removeEventListener('compositionend', onCompositionEnd);
		textarea.removeEventListener('input', onInput);
	};
}
