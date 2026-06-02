/** Maps Trello label color names to hex values matching Trello's palette. */
const TRELLO_COLORS: Record<string, string> = {
	green: '#61bd4f',
	yellow: '#f2d600',
	orange: '#ff9f1a',
	red: '#eb5a46',
	purple: '#c377e0',
	blue: '#0079bf',
	sky: '#00c2e0',
	lime: '#51e898',
	pink: '#ff78cb',
	black: '#344563',
	green_dark: '#519839',
	yellow_dark: '#d9b51c',
	orange_dark: '#cd8313',
	red_dark: '#b04632',
	purple_dark: '#89609e',
	blue_dark: '#055a8c',
	sky_dark: '#0098b7',
	lime_dark: '#4bbf6b',
	pink_dark: '#ef7564',
	black_dark: '#091e42',
	green_light: '#b3ecb3',
	yellow_light: '#fce8a0',
	orange_light: '#fdd0a0',
	red_light: '#f5c0b8',
	purple_light: '#dfc0eb',
	blue_light: '#bcd9ea',
	sky_light: '#b3e4f0',
	lime_light: '#b3f0c0',
	pink_light: '#f0c0e0',
	black_light: '#8993a4'
};

export function trelloLabelColor(color: string | null | undefined): string {
	if (!color) return '#888';
	return TRELLO_COLORS[color] ?? color;
}
