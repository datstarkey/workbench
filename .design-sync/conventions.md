# Workbench design system (tokens only)

Workbench is a dense, dark, IDE-style desktop app. Its real components are Svelte (shadcn-svelte + bits-ui), so **this project ships no React components**: build with plain React elements styled by the stylesheet below. `window.WorkbenchTheme` is intentionally empty.

## Setup

- Dark mode is always on. Put `class="dark"` on `<html>` (`document.documentElement.classList.add('dark')`), not on a wrapper div: the `--wb-*` chrome tokens are only defined under `.dark`, and accent presets only apply on `:root`.
- Accent is user-selectable: set `document.documentElement.dataset.accent` to `tideline`, `ember`, `moss` or `iris` (omit for the default violet). It drives `--wb-accent`, `--primary` and `--ring`.
- `html, body` are `height: 100%; overflow: hidden` (full-window app). Make the root a full-height flex layout and give scrolling regions `overflow-auto`.
- Font: system UI stack for chrome, `font-mono` for paths, branches, counters and status text. No web fonts.

## Styling idiom: Tailwind v4 utilities, compiled

`styles.css` is the app's real compiled Tailwind output. **Only classes already used by the app exist** (it is not a Tailwind runtime). Anything not listed there: use an inline `style` with the CSS variables, e.g. `style={{ background: 'var(--wb-panel2)' }}`.

| Family           | Classes                                                                                                                                                                                         |
| ---------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Surfaces         | `bg-wb-rail` (darkest: rails, status bar), `bg-wb-bg` (canvas), `bg-wb-panel` (sidebars), `bg-wb-panel2` (raised rows, avatars)                                                                 |
| Text             | `text-wb-ink` (primary), `text-wb-ink-mute` (secondary), `text-wb-ink-soft` (tertiary/meta)                                                                                                     |
| Lines            | `border-wb-hair`, `border-wb-hair-soft` (1px hairlines between regions)                                                                                                                         |
| Accent           | `bg-wb-accent`, `bg-wb-accent-soft`, `text-wb-accent`, `border-wb-accent`                                                                                                                       |
| Session / status | `text-wb-claude` `text-wb-codex` `text-wb-shell` `text-wb-ok` `text-wb-warn` `text-wb-err` (and matching `bg-wb-*`; tints `bg-wb-claude/20`, `bg-wb-codex/20`, `bg-wb-warn/20`, `bg-wb-err/10`) |
| Type scale       | `text-[9px]` … `text-[15px]`; chrome text is small: `text-[10.5px]` status, `text-[11px]`–`text-[12.5px]` lists, `text-[13px]` body                                                             |
| shadcn tokens    | `bg-background` `bg-card` `bg-muted` `bg-primary` `text-foreground` `text-muted-foreground` `text-primary-foreground` `border-input` (dialogs, forms, buttons)                                  |

Raw variables: `--wb-bg --wb-panel --wb-panel2 --wb-rail --wb-ink --wb-ink-mute --wb-ink-soft --wb-hair --wb-hair-soft --wb-claude --wb-codex --wb-shell --wb-ok --wb-warn --wb-err --wb-accent --wb-accent-soft --wb-accent-ink`, plus shadcn `--background --foreground --primary --muted --border --input --ring --radius`.

Use `--wb-*` for app chrome (rails, sidebars, tabs, status). Use shadcn tokens for dialogs, inputs and buttons. Colour sessions consistently: Claude = `wb-claude`, Codex = `wb-codex`, plain shell = `wb-shell`.

## Where the truth lives

- `styles.css` → `_ds_bundle.css`: every token and class. Read it before styling.
- `guidelines/components.md`: exact class recipes for Button, Badge, Input, Card, Tabs, Switch, Checkbox, Dialog. Copy those strings to match the real components.

## Example: status bar (from the app)

```jsx
<footer className="flex h-[22px] flex-shrink-0 items-center gap-4 border-t border-wb-hair bg-wb-rail px-2.5 font-mono text-[10.5px] text-wb-ink-mute">
	<span className="flex items-center gap-1.5 text-wb-accent">main</span>
	<span className="text-wb-warn">● 3 modified</span>
	<span className="flex-1" />
	<span className="text-wb-claude">● 2 Claude</span>
	<span className="text-wb-codex">● 1 Codex</span>
	<span className="text-wb-ink-soft">workbench v0.29.1</span>
</footer>
```
