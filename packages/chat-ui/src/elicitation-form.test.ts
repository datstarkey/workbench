import { describe, expect, it } from 'vitest';
import { elicitationContent, elicitationFields, initialDraft } from './elicitation-form';

const schema = {
	type: 'object',
	properties: {
		env: {
			type: 'string',
			title: 'Environment',
			enum: ['staging', 'prod'],
			enumNames: ['Staging']
		},
		replicas: { type: 'integer', minimum: 1, maximum: 5, default: 2 },
		ratio: { type: 'number' },
		confirm: { type: 'boolean', title: 'I understand' },
		email: { type: 'string', format: 'email' },
		name: { type: 'string', minLength: 2, maxLength: 4 },
		tier: { type: 'string', oneOf: [{ const: 'a', title: 'Tier A' }, { const: 'b' }] },
		tags: { type: 'array', items: { enum: ['x', 'y'] }, maxItems: 1 },
		nested: { type: 'object' }
	},
	required: ['env', 'replicas']
};

describe('elicitationFields', () => {
	it('reads every primitive kind and skips the rest', () => {
		const fields = elicitationFields(schema);
		expect(fields.map((f) => [f.name, f.type, f.required])).toEqual([
			['env', 'select', true],
			['replicas', 'integer', true],
			['ratio', 'number', false],
			['confirm', 'boolean', false],
			['email', 'string', false],
			['name', 'string', false],
			['tier', 'select', false],
			['tags', 'multiselect', false]
		]);
		expect(fields[0].options).toEqual([
			{ value: 'staging', label: 'Staging' },
			{ value: 'prod', label: 'prod' }
		]);
		expect(fields[6].options[0]).toEqual({ value: 'a', label: 'Tier A' });
		expect(fields[0].label).toBe('Environment');
		expect(fields[2].label).toBe('ratio');
	});

	it('is empty without properties', () => {
		expect(elicitationFields(undefined)).toEqual([]);
		expect(elicitationFields({ type: 'object' })).toEqual([]);
	});
});

describe('elicitationContent', () => {
	const fields = elicitationFields(schema);

	it('types each answer as the schema says and leaves blanks out', () => {
		const draft = { ...initialDraft(schema, fields), env: 'prod', ratio: ' 1.5 ', confirm: true };
		expect(elicitationContent(fields, draft)).toEqual({
			content: { env: 'prod', replicas: 2, ratio: 1.5, confirm: true },
			errors: {}
		});
	});

	it('reports what stops the form being sent', () => {
		const draft = {
			env: '',
			replicas: '2.5',
			ratio: 'abc',
			confirm: false,
			email: 'nope',
			name: 'toolong',
			tier: 'z',
			tags: ['x', 'y']
		};
		expect(elicitationContent(fields, draft).errors).toEqual({
			env: 'Required',
			replicas: 'Enter a whole number',
			ratio: 'Enter a number',
			email: 'Enter an email address',
			name: 'At most 4 characters',
			tier: 'Pick one of the options',
			tags: 'Pick at most 1'
		});
		expect(elicitationContent(fields, { ...draft, replicas: '9' }).errors.replicas).toBe(
			'At most 5'
		);
	});

	it('normalises date-times to ISO', () => {
		const f = elicitationFields({
			properties: { at: { type: 'string', format: 'date-time' } }
		});
		const { content } = elicitationContent(f, { at: '2026-10-04T12:00:00Z' });
		expect(content.at).toBe('2026-10-04T12:00:00.000Z');
	});
});
