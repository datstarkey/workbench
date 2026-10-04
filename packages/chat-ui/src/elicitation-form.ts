import type { TranscriptItem } from '@workbench/types';

/** An MCP server asking for input: a form (`schema`) or a page to open (`url`). */
export type ElicitationItem = Extract<TranscriptItem, { kind: 'elicitation' }>;

export type ElicitationValue = string | number | boolean | string[];

export interface ElicitationOption {
	value: string;
	label: string;
}

/** One field of an MCP elicitation form (the spec allows only flat primitives). */
export interface ElicitationField {
	name: string;
	label: string;
	description?: string;
	required: boolean;
	type: 'string' | 'number' | 'integer' | 'boolean' | 'select' | 'multiselect';
	format?: string;
	options: ElicitationOption[];
	min?: number;
	max?: number;
	minLength?: number;
	maxLength?: number;
}

/** What the inputs hold: text for text/number fields, booleans, picked values. */
export type ElicitationDraft = Record<string, string | boolean | string[]>;

type Json = Record<string, unknown>;

const isObject = (v: unknown): v is Json =>
	typeof v === 'object' && v !== null && !Array.isArray(v);
const num = (v: unknown) => (typeof v === 'number' ? v : undefined);
const str = (v: unknown) => (typeof v === 'string' ? v : undefined);

/** `enum` (+ legacy `enumNames`) or `oneOf`/`anyOf` of `{const, title}`. */
function options(prop: Json): ElicitationOption[] {
	if (Array.isArray(prop.enum)) {
		const names = Array.isArray(prop.enumNames) ? prop.enumNames : [];
		return prop.enum
			.filter((v): v is string => typeof v === 'string')
			.map((value, i) => ({ value, label: str(names[i]) ?? value }));
	}
	const list = Array.isArray(prop.oneOf) ? prop.oneOf : Array.isArray(prop.anyOf) ? prop.anyOf : [];
	return list.filter(isObject).flatMap((o) => {
		const value = str(o.const);
		return value === undefined ? [] : [{ value, label: str(o.title) ?? value }];
	});
}

function field(name: string, prop: Json, required: boolean): ElicitationField | null {
	const base = {
		name,
		label: str(prop.title) ?? name,
		description: str(prop.description),
		required,
		options: [] as ElicitationOption[]
	};
	switch (prop.type) {
		case 'string': {
			const opts = options(prop);
			if (opts.length > 0) return { ...base, type: 'select', options: opts };
			return {
				...base,
				type: 'string',
				format: str(prop.format),
				minLength: num(prop.minLength),
				maxLength: num(prop.maxLength)
			};
		}
		case 'number':
		case 'integer':
			return { ...base, type: prop.type, min: num(prop.minimum), max: num(prop.maximum) };
		case 'boolean':
			return { ...base, type: 'boolean' };
		case 'array': {
			const opts = isObject(prop.items) ? options(prop.items) : [];
			if (opts.length === 0) return null;
			return {
				...base,
				type: 'multiselect',
				options: opts,
				min: num(prop.minItems),
				max: num(prop.maxItems)
			};
		}
		default:
			return null;
	}
}

/** The form's fields in schema order; kinds it can't render are left out. */
export function elicitationFields(schema: Record<string, unknown> | undefined): ElicitationField[] {
	if (!schema || !isObject(schema.properties)) return [];
	const required = new Set(Array.isArray(schema.required) ? schema.required : []);
	return Object.entries(schema.properties).flatMap(([name, prop]) => {
		const f = isObject(prop) ? field(name, prop, required.has(name)) : null;
		return f ? [f] : [];
	});
}

/** Inputs seeded from each field's schema `default`. */
export function initialDraft(
	schema: Record<string, unknown> | undefined,
	fields: ElicitationField[]
): ElicitationDraft {
	const props = isObject(schema?.properties) ? schema.properties : {};
	return Object.fromEntries(
		fields.map((f) => {
			const d = isObject(props[f.name]) ? (props[f.name] as Json).default : undefined;
			if (f.type === 'boolean') return [f.name, d === true];
			if (f.type === 'multiselect')
				return [f.name, Array.isArray(d) ? d.filter((v) => typeof v === 'string') : []];
			return [f.name, typeof d === 'string' || typeof d === 'number' ? String(d) : ''];
		})
	);
}

const EMAIL = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
const DATE = /^\d{4}-\d{2}-\d{2}$/;

type Parsed = { value?: ElicitationValue; error?: string };

const ok = (value: ElicitationValue): Parsed => ({ value });
const bad = (error: string): Parsed => ({ error });

function parseText(f: ElicitationField, raw: string): Parsed {
	const text = raw.trim();
	if (f.type === 'number' || f.type === 'integer') {
		const n = Number(text);
		if (!Number.isFinite(n)) return bad('Enter a number');
		if (f.type === 'integer' && !Number.isInteger(n)) return bad('Enter a whole number');
		if (f.min !== undefined && n < f.min) return bad(`At least ${f.min}`);
		if (f.max !== undefined && n > f.max) return bad(`At most ${f.max}`);
		return ok(n);
	}
	if (f.type === 'select') {
		return f.options.some((o) => o.value === raw) ? ok(raw) : bad('Pick one of the options');
	}
	if (f.minLength !== undefined && raw.length < f.minLength) {
		return bad(`At least ${f.minLength} characters`);
	}
	if (f.maxLength !== undefined && raw.length > f.maxLength) {
		return bad(`At most ${f.maxLength} characters`);
	}
	switch (f.format) {
		case 'email':
			return EMAIL.test(text) ? ok(text) : bad('Enter an email address');
		case 'uri':
			return URL.canParse(text) ? ok(text) : bad('Enter a full URL');
		case 'date':
			return DATE.test(text) ? ok(text) : bad('Enter a date');
		case 'date-time': {
			const at = Date.parse(text);
			return Number.isNaN(at) ? bad('Enter a date and time') : ok(new Date(at).toISOString());
		}
		default:
			return ok(raw);
	}
}

function parse(f: ElicitationField, raw: string | boolean | string[] | undefined): Parsed {
	if (f.type === 'boolean') return ok(raw === true);
	if (f.type === 'multiselect') {
		const picked = (Array.isArray(raw) ? raw : []).filter((v) =>
			f.options.some((o) => o.value === v)
		);
		if (picked.length === 0 && !f.required) return {};
		if (f.min !== undefined && picked.length < f.min) return bad(`Pick at least ${f.min}`);
		if (f.max !== undefined && picked.length > f.max) return bad(`Pick at most ${f.max}`);
		if (picked.length === 0) return bad('Pick at least one');
		return ok(picked);
	}
	const text = typeof raw === 'string' ? raw : '';
	if (text.trim() === '') return f.required ? bad('Required') : {};
	return parseText(f, text);
}

/**
 * The content to send for an accepted form, typed as the schema says, or the
 * per-field errors that stop it being sent.
 */
export function elicitationContent(
	fields: ElicitationField[],
	draft: ElicitationDraft
): { content: Record<string, ElicitationValue>; errors: Record<string, string> } {
	const content: Record<string, ElicitationValue> = {};
	const errors: Record<string, string> = {};
	for (const f of fields) {
		const { value, error } = parse(f, draft[f.name]);
		if (error) errors[f.name] = error;
		else if (value !== undefined) content[f.name] = value;
	}
	return { content, errors };
}

/** The URL to open, only if it's http(s): a server-sent link must not run script. */
export function elicitationUrl(item: ElicitationItem): string | null {
	if (!item.url || !URL.canParse(item.url)) return null;
	const { protocol } = new URL(item.url);
	return protocol === 'https:' || protocol === 'http:' ? item.url : null;
}
