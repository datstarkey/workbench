import { parsePairingUri, type PairingInfo } from '@workbench/transport';
import * as barcodeScanner from '@tauri-apps/plugin-barcode-scanner';
import { onBackButtonPress } from '@tauri-apps/api/app';
import type { PluginListener } from '@tauri-apps/api/core';

/** The plugin surface pairing uses (injectable for tests). */
export type QrScanner = Pick<
	typeof barcodeScanner,
	'checkPermissions' | 'requestPermissions' | 'scan' | 'cancel'
> & { onBackButtonPress: typeof onBackButtonPress };

const defaultScanner: QrScanner = { ...barcodeScanner, onBackButtonPress };

export const CAMERA_DENIED =
	'Camera permission denied. Allow it in system settings, or enter the server details below.';
export const NOT_A_PAIRING_CODE = 'Not a Workbench pairing code';

/**
 * Scans the desktop's pairing QR code (Settings → Server mode → Pair phone).
 *
 * The scan is windowed: the camera renders behind the (transparent) webview so
 * our overlay can offer Cancel. Cancel and the Android back button end the scan
 * here rather than waiting on the plugin, whose promise never settles once
 * cancelled (nor on a device without a camera).
 */
export class PairingScan {
	scanning = $state(false);

	private readonly scanner: QrScanner;
	/** Bumped on every scan start and cancel; a scan whose number is stale is ignored. */
	private generation = 0;
	private backButton: PluginListener | null = null;

	constructor(scanner: QrScanner = defaultScanner) {
		this.scanner = scanner;
	}

	/** The scanned pairing; null when cancelled, already scanning, or superseded. Throws a readable error. */
	async scan(): Promise<PairingInfo | null> {
		if (this.scanning) return null;
		this.scanning = true;
		const generation = ++this.generation;
		const stale = () => generation !== this.generation;
		try {
			let permission = await this.scanner.checkPermissions();
			if (permission !== 'granted') permission = await this.scanner.requestPermissions();
			if (stale()) return null;
			if (permission !== 'granted') throw new Error(CAMERA_DENIED);
			const listener = await this.scanner.onBackButtonPress(() => void this.cancel());
			if (stale()) {
				void listener.unregister();
				return null;
			}
			this.backButton = listener;
			const { content } = await this.scanner.scan({
				windowed: true,
				formats: [barcodeScanner.Format.QRCode]
			});
			if (stale()) return null;
			const pairing = parsePairingUri(content);
			if (!pairing) throw new Error(NOT_A_PAIRING_CODE);
			return pairing;
		} catch (e) {
			if (stale()) return null;
			const message = e instanceof Error ? e.message : String(e);
			if (/cancel/i.test(message)) return null;
			throw e instanceof Error ? e : new Error(message);
		} finally {
			if (!stale()) this.end();
		}
	}

	/** Overlay Cancel / Android back: end the scan now; a late result is ignored. */
	cancel = async (): Promise<void> => {
		if (!this.scanning) return;
		this.generation++;
		this.end();
		try {
			await this.scanner.cancel();
		} catch {
			/* nothing was scanning on the native side */
		}
	};

	private end(): void {
		this.scanning = false;
		void this.backButton?.unregister();
		this.backButton = null;
	}
}
