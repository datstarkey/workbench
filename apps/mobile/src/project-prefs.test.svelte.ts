import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { ProjectPrefs } from './project-prefs.svelte';
import { stubLocalStorage } from './test-helpers';

beforeEach(stubLocalStorage);
afterEach(() => vi.unstubAllGlobals());

it('keeps favourites and collapsed sections per machine across relaunch', () => {
	const prefs = new ProjectPrefs('mac');
	prefs.toggleFavourite('/repo');
	prefs.toggleSection('group:Web');
	expect(new ProjectPrefs('mac').favourites.has('/repo')).toBe(true);
	expect(new ProjectPrefs('mac').collapsed.has('group:Web')).toBe(true);
	expect(new ProjectPrefs('pc').favourites.size).toBe(0);

	prefs.toggleFavourite('/repo');
	expect(new ProjectPrefs('mac').favourites.has('/repo')).toBe(false);
});

it('ignores a corrupt saved value', () => {
	localStorage.setItem('wb.favourites.mac', '{nope');
	expect(new ProjectPrefs('mac').favourites.size).toBe(0);
});
