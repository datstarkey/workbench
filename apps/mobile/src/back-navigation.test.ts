import { expect, it, vi } from 'vitest';
import { BackNavigation } from './back-navigation';

it('dismisses sheets before their screen and unregisters at Home', async () => {
	let press = () => {};
	const unregister = vi.fn();
	const register = vi.fn(async (handler: () => void) => {
		press = handler;
		return unregister;
	});
	const navigation = new BackNavigation(register);
	const screen = vi.fn();
	const sheet = vi.fn();
	// Children mount before parents; sheet priority must win in either order.
	const dismiss = navigation.add(sheet, 1);
	const leave = navigation.add(screen);
	await Promise.resolve();
	press();
	expect(sheet).toHaveBeenCalledOnce();
	expect(screen).not.toHaveBeenCalled();
	dismiss();
	press();
	expect(screen).toHaveBeenCalledOnce();
	leave();
	expect(unregister).toHaveBeenCalledOnce();
	expect(register).toHaveBeenCalledOnce();
});

it('removes a listener whose registration finished after the screen closed', async () => {
	let resolve!: (remove: () => void) => void;
	const navigation = new BackNavigation(
		() =>
			new Promise((r) => {
				resolve = r;
			})
	);
	const leave = navigation.add(() => {});
	leave();
	const remove = vi.fn();
	resolve(remove);
	await Promise.resolve();
	expect(remove).toHaveBeenCalledOnce();
});
