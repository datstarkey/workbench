import { encode } from 'uqr';
import { buildPairingUri } from '@workbench/transport';
import type { PairingAddress } from '$lib/server-mode';

/** Port from a bound socket address (`0.0.0.0:4317`, `[::]:4317`), or null. */
export function boundPort(address: string | null): number | null {
	const port = Number(address?.match(/:(\d+)$/)?.[1]);
	return Number.isInteger(port) && port > 0 && port <= 65535 ? port : null;
}

/** The address pre-selected in the pairing dialog (already ranked Tailscale-first by Rust). */
export function defaultPairingAddress(addresses: PairingAddress[]): string | undefined {
	return addresses[0]?.address;
}

export function pairingUrl(host: string, port: number): string {
	return `http://${host}:${port}`;
}

export interface PairingQr {
	/** Modules per side, including the quiet-zone border. */
	size: number;
	/** SVG path drawing every dark module as a unit square. */
	path: string;
}

/** QR code for the pairing URI, as data for an inline `<svg viewBox>`. */
export function pairingQr(host: string, port: number, token: string): PairingQr {
	const { data } = encode(buildPairingUri({ url: pairingUrl(host, port), token }), { border: 2 });
	let path = '';
	data.forEach((row, y) =>
		row.forEach((dark, x) => {
			if (dark) path += `M${x} ${y}h1v1h-1z`;
		})
	);
	return { size: data.length, path };
}
