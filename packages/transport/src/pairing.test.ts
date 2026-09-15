import { describe, expect, it } from 'vitest';
import { buildPairingUri, isStrongToken, pairingQuery, parsePairingUri } from './pairing.ts';

const TOKEN = '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef';
const URL_ = 'http://100.64.1.2:4317';

const uri = (params: Record<string, string>, base = 'workbench://pair') =>
	`${base}?${new URLSearchParams(params)}`;

describe('pairing URI', () => {
	it('round-trips a url and token', () => {
		const built = buildPairingUri({ url: URL_, token: TOKEN });
		expect(built.startsWith('workbench://pair?v=1&url=http%3A%2F%2F')).toBe(true);
		expect(parsePairingUri(built)).toEqual({ url: URL_, token: TOKEN });
	});

	it('accepts https, a trailing slash and surrounding whitespace', () => {
		const built = buildPairingUri({ url: 'https://box.tail1234.ts.net/', token: TOKEN });
		expect(parsePairingUri(`  ${built}\n`)).toEqual({
			url: 'https://box.tail1234.ts.net',
			token: TOKEN
		});
	});

	it.each([
		['not a url', 'hello world'],
		['wrong scheme', uri({ v: '1', url: URL_, token: TOKEN }, 'https://pair')],
		['wrong host/path', uri({ v: '1', url: URL_, token: TOKEN }, 'workbench://connect')],
		['extra path', uri({ v: '1', url: URL_, token: TOKEN }, 'workbench://pair/x')],
		['longer host', uri({ v: '1', url: URL_, token: TOKEN }, 'workbench://pairx')],
		[
			'trailing slash before the query',
			uri({ v: '1', url: URL_, token: TOKEN }, 'workbench://pair/')
		],
		['no authority slashes', uri({ v: '1', url: URL_, token: TOKEN }, 'workbench:pair')],
		['uppercase scheme', uri({ v: '1', url: URL_, token: TOKEN }, 'WORKBENCH://pair')],
		['a fragment', `${buildPairingUri({ url: URL_, token: TOKEN })}#x`],
		['a duplicated version', `${uri({ v: '1', url: URL_, token: TOKEN })}&v=1`],
		[
			'a duplicated url',
			`${uri({ v: '1', url: URL_, token: TOKEN })}&url=${encodeURIComponent('http://evil:1')}`
		],
		['a duplicated token', `${uri({ v: '1', url: URL_, token: TOKEN })}&token=${TOKEN}`],
		['missing version', uri({ url: URL_, token: TOKEN })],
		['other version', uri({ v: '2', url: URL_, token: TOKEN })],
		['missing url', uri({ v: '1', token: TOKEN })],
		['non-http url', uri({ v: '1', url: 'ftp://box:4317', token: TOKEN })],
		['javascript url', uri({ v: '1', url: 'javascript:alert(1)', token: TOKEN })],
		['url without host', uri({ v: '1', url: 'http://', token: TOKEN })],
		['url with credentials', uri({ v: '1', url: 'http://u:p@box:4317', token: TOKEN })],
		['url with a path', uri({ v: '1', url: 'http://box:4317/api', token: TOKEN })],
		['url with a query', uri({ v: '1', url: 'http://box:4317/?x=1', token: TOKEN })],
		['url with an empty query', uri({ v: '1', url: 'http://box:4317?', token: TOKEN })],
		['url with a fragment', uri({ v: '1', url: 'http://box:4317/#x', token: TOKEN })],
		['missing token', uri({ v: '1', url: URL_ })],
		['short token', uri({ v: '1', url: URL_, token: 'secret' })],
		['token with whitespace', uri({ v: '1', url: URL_, token: `${TOKEN.slice(0, 40)} x` })]
	])('rejects %s', (_, text) => {
		expect(parsePairingUri(text)).toBeNull();
	});
});

describe('pairingQuery', () => {
	it('extracts the query after the exact prefix without URL-parsing the custom scheme', () => {
		const query = pairingQuery(' workbench://pair?v=1&url=http%3A%2F%2Fbox%3A1&token=t \n');
		expect(query?.get('v')).toBe('1');
		expect(query?.get('url')).toBe('http://box:1');
		expect(query?.get('token')).toBe('t');
	});

	it.each([
		'workbench://pairx?v=1',
		'workbench://pair/?v=1',
		'workbench:pair?v=1',
		'workbench://pair?v=1#f'
	])('rejects %s', (text) => {
		expect(pairingQuery(text)).toBeNull();
	});
});

describe('isStrongToken', () => {
	it('matches the Rust rule', () => {
		expect(isStrongToken('a'.repeat(32))).toBe(true);
		expect(isStrongToken('a'.repeat(31))).toBe(false);
		expect(isStrongToken(`${'a'.repeat(32)} `)).toBe(false);
	});
});
