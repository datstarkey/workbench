# design-sync notes

## Why this is a tokens-only sync

- `@workbench/ui` is Svelte 5 (shadcn-svelte + bits-ui). Claude Design renders React, so the converter cannot bundle it. The user chose to ship the look only: compiled CSS, a conventions header and component class recipes. `window.WorkbenchTheme` is empty on purpose.
- The synced "package" is `.design-sync/theme-entry/` (a stub `index.js` + `package.json`), because `cssEntry` must sit inside the package dir. It is not a workspace package; bun never sees it.
- `workbench.css` is the desktop app's real compiled Tailwind output (`apps/desktop/dist/assets/index-*.css`), copied by `buildCmd`. It is gitignored. `App-*.css` (xterm + scoped Svelte styles) is deliberately not shipped.

## Running it

- `buildCmd` first (Vite build + copy), then the converter with `--node-modules ./.ds-sync/node_modules`. That dir needs `react@18 react-dom@18` installed next to `esbuild ts-morph @types/react playwright`: the converter vendors React even for a tokens-only DS.
- `[DTS_REACT]` during build is harmless (zero components).
- Validate with `--no-render-check`: there are no previews to render, and the Claude Code sandbox blocks the local port the render check binds (`listen EPERM`).
- `[TOKENS_MISSING]` for `--bits-*` variables is expected: bits-ui sets them inline at runtime.

## Re-sync risks

- The compiled CSS contains only utilities the app currently uses. If a class named in `conventions.md` or `guidelines/components.md` is removed from the app, it silently stops resolving in designs. Re-run the class check (every backticked class and every `className` in the snippet must appear in `workbench.css`) on each sync.
- `guidelines/components.md` is hand-copied from `packages/ui/src/ui/*`. It drifts when those `tv()`/`cn()` class strings change. Re-diff on each sync.
- The status-bar snippet in `conventions.md` mirrors `apps/desktop/src/lib/features/chrome/StatusBar.svelte`.
- If `@workbench/theme` gains new accent presets or tokens, update the Setup and token lists in `conventions.md`.
