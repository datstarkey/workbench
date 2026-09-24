import { browser, $, expect } from '@wdio/globals';

// Smoke checks that the built app boots and its main surfaces render. Not a
// behaviour suite: logic is covered by the store/unit tests.
describe('Workbench smoke', () => {
	it('boots the main window with the activity rail', async () => {
		await expect(browser).toHaveTitle('Workbench');
		await expect($('nav [aria-label="Projects"]')).toBeDisplayed();
		await expect($('nav [aria-label="Settings"]')).toBeDisplayed();
	});

	it('lists the seeded project in the sidebar', async () => {
		await expect($('aside').$('button=workbench')).toBeDisplayed();
	});

	it('opens the project and shows its package.json scripts', async () => {
		await $('aside').$('button=workbench').click();
		await expect($('[aria-label="Expand workbench"], [aria-label="Collapse workbench"]')).toExist();

		await $('nav [aria-label="GitHub"]').click();
		await $('button=Scripts').click();

		await expect($('button[aria-label="Run build"]')).toBeDisplayed();
		await expect($('button=Install')).toBeDisplayed();
	});

	it('opens settings in its own window', async () => {
		const main = await browser.getWindowHandle();
		await $('nav [aria-label="Settings"]').click();

		await browser.waitUntil(async () => (await browser.getWindowHandles()).length > 1, {
			timeoutMsg: 'settings window did not open'
		});
		const settings = (await browser.getWindowHandles()).find((h) => h !== main)!;
		await browser.switchToWindow(settings);

		const sections = $('[role="tablist"][aria-label="Settings sections"]');
		await expect(sections).toBeDisplayed();
		await sections.$('button*=Claude Code').click();
		await expect($('[role="tablist"][aria-label="Claude Code sections"]')).toBeDisplayed();

		await browser.switchToWindow(main);
	});
});
