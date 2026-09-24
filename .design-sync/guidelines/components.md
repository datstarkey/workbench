# Component class recipes

Class strings copied from the Svelte components in `packages/ui/src/ui/`. Reproduce a component in React by putting the same classes on the same element. `focus-visible:*`, `aria-invalid:*` and `data-[state=*]` variants are included where the compiled stylesheet carries them.

## Button (`<button>`)

Base: `inline-flex shrink-0 items-center justify-center gap-2 rounded-md text-sm font-medium whitespace-nowrap transition-all outline-none disabled:pointer-events-none disabled:opacity-50`

| Variant     | Classes                                                                                                                                 |
| ----------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| default     | `bg-primary text-primary-foreground hover:bg-primary/90 shadow-xs`                                                                      |
| destructive | `bg-destructive hover:bg-destructive/90 dark:bg-destructive/60 text-white shadow-xs`                                                    |
| outline     | `bg-background hover:bg-accent hover:text-accent-foreground dark:bg-input/30 dark:border-input dark:hover:bg-input/50 border shadow-xs` |
| secondary   | `bg-secondary text-secondary-foreground hover:bg-secondary/80 shadow-xs`                                                                |
| ghost       | `hover:bg-accent hover:text-accent-foreground dark:hover:bg-accent/50`                                                                  |
| link        | `text-primary underline-offset-4 hover:underline`                                                                                       |

| Size                     | Classes                         |
| ------------------------ | ------------------------------- |
| default                  | `h-9 px-4 py-2`                 |
| sm                       | `h-8 gap-1.5 rounded-md px-3`   |
| lg                       | `h-10 rounded-md px-6`          |
| icon / icon-sm / icon-lg | `size-9` / `size-8` / `size-10` |

## Badge (`<span>`)

Base: `inline-flex w-fit shrink-0 items-center justify-center gap-1 overflow-hidden rounded-full border px-2 py-0.5 text-xs font-medium whitespace-nowrap`

Variants: default `bg-primary text-primary-foreground border-transparent`; secondary `bg-secondary text-secondary-foreground border-transparent`; destructive `bg-destructive dark:bg-destructive/70 border-transparent text-white`; outline `text-foreground`.

## Input (`<input>`)

`flex h-9 w-full min-w-0 rounded-md border border-input bg-background px-3 py-1 text-base shadow-xs outline-none placeholder:text-muted-foreground disabled:cursor-not-allowed disabled:opacity-50 md:text-sm dark:bg-input/30 focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50`

## Card (`<div>`)

`flex flex-col gap-6 rounded-xl border bg-card py-6 text-card-foreground shadow-sm`

## Tabs

- List: `inline-flex h-9 w-fit items-center justify-center rounded-lg bg-muted p-[3px] text-muted-foreground`
- Trigger (`<button>`, set `data-state="active"` on the selected one): `inline-flex h-[calc(100%-1px)] flex-1 items-center justify-center gap-1.5 rounded-md border border-transparent px-2 py-1 text-sm font-medium whitespace-nowrap text-foreground dark:text-muted-foreground data-[state=active]:bg-background data-[state=active]:shadow-sm dark:data-[state=active]:border-input dark:data-[state=active]:bg-input/30 dark:data-[state=active]:text-foreground`

## Switch (`<button role="switch" data-state="checked|unchecked">`)

- Track: `peer inline-flex h-[1.15rem] w-8 shrink-0 items-center rounded-full border border-transparent shadow-xs transition-all outline-none data-[state=checked]:bg-primary data-[state=unchecked]:bg-input dark:data-[state=unchecked]:bg-input/80`
- Thumb (`<span>`, same `data-state`): `pointer-events-none block size-4 rounded-full bg-background ring-0 transition-transform data-[state=checked]:translate-x-[calc(100%-2px)] data-[state=unchecked]:translate-x-0 dark:data-[state=checked]:bg-primary-foreground dark:data-[state=unchecked]:bg-foreground`

## Checkbox (`<button role="checkbox" data-state="checked|unchecked">`)

`peer flex size-4 shrink-0 items-center justify-center rounded-[4px] border border-input shadow-xs outline-none data-[state=checked]:border-primary data-[state=checked]:bg-primary data-[state=checked]:text-primary-foreground dark:bg-input/30`

## Dialog content (`<div role="dialog">`, over a dimmed overlay)

`fixed top-[50%] left-[50%] z-50 grid w-full max-w-[calc(100%-2rem)] translate-x-[-50%] translate-y-[-50%] gap-4 rounded-lg border bg-background p-6 shadow-lg sm:max-w-lg`
