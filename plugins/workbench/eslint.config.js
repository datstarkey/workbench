import js from '@eslint/js';
import prettier from 'eslint-config-prettier';
import { defineConfig } from 'eslint/config';
import ts from 'typescript-eslint';

export default defineConfig(
	{ ignores: ['types/'] },
	js.configs.recommended,
	...ts.configs.recommended,
	prettier,
	// The engine's globals (`h`, web APIs) are declared in types/, which tsc checks.
	{
		rules: {
			'no-undef': 'off',
			// Destructuring drops fields from a hook's input this way.
			'@typescript-eslint/no-unused-vars': ['error', { ignoreRestSiblings: true }]
		}
	}
);
