import { browser, $, $$, expect } from '@wdio/globals';

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
		// The sidebar pane animates open, so a click can land before its tab bar
		// settles and miss the Scripts tab; keep clicking until the scripts list.
		const build = $('button[aria-label="Run build"]');
		await browser.waitUntil(
			async () => {
				if (await build.isDisplayed()) return true;
				await $('button=Scripts').click();
				return build.isDisplayed();
			},
			{ interval: 1_000, timeoutMsg: 'the Scripts tab never listed the build script' }
		);
		await expect($('button=Install')).toBeDisplayed();
	});

	it('opens a terminal tab the server runs, and closes it', async () => {
		const tabs = $$('[role="tab"]');
		const before = await tabs.length;
		await $('button[aria-label="New terminal"], button*=New Terminal').click();
		// The tab appears once the server's workspace snapshot includes it.
		await browser.waitUntil(async () => (await $$('[role="tab"]').length) === before + 1, {
			timeoutMsg: 'the new terminal tab never appeared'
		});
		const tab = $$('[role="tab"]')[before];
		await expect(tab).toHaveText(expect.stringContaining('Terminal'));
		await expect($('.xterm')).toBeDisplayed({ wait: 15_000 });

		const close = tab.parentElement().$('button[aria-label="Close terminal tab"]');
		await tab.moveTo();
		await close.click();
		await browser.waitUntil(async () => (await $$('[role="tab"]').length) === before, {
			timeoutMsg: 'the closed terminal tab stayed'
		});
	});

	it('opens settings in its own window', async () => {
		const main = await browser.getWindowHandle();
		await $('nav [aria-label="Settings"]').click();

		await browser.waitUntil(async () => (await browser.getWindowHandles()).length > 1, {
			timeoutMsg: 'settings window did not open'
		});
		const settings = (await browser.getWindowHandles()).find((h) => h !== main)!;
		await browser.switchToWindow(settings);

		const sections = $('nav[aria-label="Settings sections"]');
		// A fresh WebKitGTK window can take longer than expect's 3s default to load.
		await expect(sections).toBeDisplayed({ wait: 15_000 });
		await sections.$('button=Permissions').click();
		await expect($('h1=Permissions')).toBeDisplayed();
		await expect($('[role="group"][aria-label="Settings scope"]')).toBeDisplayed();

		await browser.switchToWindow(main);
	});
});
