# 13 — Design and accessibility parity with T3 Code

**Status:** `[ ]` proposed — this change is documentation only; no product code
**Owns:** the token mapping from T3 Code (`pingdotgg/t3code`) to `goble-ui`, and the concrete redesign of the settings pane, the composer, the tab strip and the harness window to T3's measured standards. This is the reference for D1–D15 in §4.
**Depends on:** [`../06-renderer/ui-spec/01-design-tokens.md`](../06-renderer/ui-spec/01-design-tokens.md), [`../06-renderer/ui-spec/05-chat-view.md`](../06-renderer/ui-spec/05-chat-view.md), [`../06-renderer/ui-spec/07-topbar.md`](../06-renderer/ui-spec/07-topbar.md), [`../06-renderer/ui-spec/10-settings-tab.md`](../06-renderer/ui-spec/10-settings-tab.md), [`../06-renderer/agent-tui.md`](../06-renderer/agent-tui.md)

References read for this spec:

- T3 Code at `/workspace/repos/t3code`, commit `b0f0c44d` (shallow clone). Line references below are to that checkout.
- Goble at `/workspace/repos/goble`, branch `feat/t3-design-parity`, base `16a919a` (`origin/main`).

T3 is a Tailwind v4 + Base UI application. Goble is a custom wgpu renderer. Almost nothing in T3 is copied verbatim: the values are translated into `Theme` tokens and the interaction is re-expressed in goble's explicit focus model. Where a thing cannot be translated, §2.1 says so rather than pretending.

---

## 1. What T3's system actually is

### 1.1 Stack and where the numbers live

| Layer | T3 implementation | Consequence for this spec |
|---|---|---|
| Tokens | `apps/web/src/index.css` — 275 distinct custom-property names across 587 declarations, plus Tailwind v4's own `@theme` scale | The numbers are real CSS variables, not a style guide. Every value below is read from this file. |
| Primitives | `apps/web/src/components/ui/*` — **Base UI 1.9** (`@base-ui/react`), wrapped in `cva` and `cn` | These are **not** Radix or shadcn wrappers. Base UI supplies focus trap, roving highlight, popup positioning and ARIA wiring; the classes supply the look. |
| Icons | `lucide-react` | Goble uses its own icon names; only sizes and colours map. |
| Desktop | Electron (`apps/desktop`) | Platform a11y comes from Chromium/Electron, which goble does not have. |

### 1.2 Colour roles

T3 has no single `ColorScheme` struct. It has semantic CSS variables, resolved once per light/dark theme, then run through a second contrast layer (§1.9). The load-bearing roles:

| Role | Token | Light | Dark |
|---|---|---|---|
| Canvas | `--background` | `--color-zinc-25` = `oklch(99.2% 0 0)` | `#000` |
| Text | `--foreground` | `--color-zinc-900` | `--color-neutral-100` |
| Card | `--card` | `#fff` | `color-mix(in srgb, var(--color-neutral-950) 97%, var(--color-white))` |
| Popover/menu | `--popover` | `#fff` | same mix as card |
| Primary / send | `--primary` | `oklch(0.488 0.217 264)` | `oklch(0.571 0.21 264)` |
| On primary | `--primary-foreground` | `#fff` | `--color-neutral-100` |
| Secondary fill | `--secondary` | `--color-zinc-50` | `--alpha(var(--color-white) / 3%)` |
| Muted fill | `--muted` | `--color-zinc-50` | `--alpha(var(--color-white) / 3%)` |
| Muted text | `--muted-foreground` | `--color-zinc-500` | `--color-neutral-400` |
| Placeholder | `--placeholder` | = `--muted-foreground` | = `--muted-foreground` |
| Hover/accent fill | `--accent` | `--color-zinc-100` | `--alpha(var(--color-white) / 4%)` |
| Border / rule | `--border` | `--color-zinc-200` | `--alpha(var(--color-white) / 8%)` |
| Control outline | `--input` | `--color-zinc-300` | `--alpha(var(--color-white) / 11%)` |
| Focus ring | `--ring` | `--primary` | `--primary` |
| Destructive | `--destructive` | `--color-red-500` | `color-mix(in srgb, var(--color-red-500) 90%, var(--color-white))` |
| Destructive text | `--destructive-foreground` | `--color-red-700` | `--color-red-400` |
| Success | `--success` | `--color-emerald-500` | `--color-emerald-500` |
| Success text | `--success-foreground` | `--color-emerald-700` | `--color-emerald-400` |
| Warning | `--warning` | `--color-amber-500` | `--color-amber-500` |
| Warning text | `--warning-foreground` | `--color-amber-700` | `--color-amber-400` |
| Info | `--info` | `--color-blue-500` | `--color-blue-500` |
| Info text | `--info-foreground` | `--color-blue-700` | `--color-blue-400` |
| Sidebar | `--sidebar` | `--color-zinc-50` | = `--card` |
| Sidebar hover/selected | `--sidebar-row-hover` / `--sidebar-row-selected` | zinc-25 / white | white 4% / white 3% |
| Selection | `--message-surface` | `color-mix(in srgb, var(--color-zinc-200) 55%, var(--color-zinc-100))` | neutral-950 92% + white |

T3 deliberately leaves large surfaces near-neutral and puts its brand colour on `--primary`. Status colours are paired: a saturated fill token plus a darker/lighter *foreground* token for text on that fill or on an 8%/16% tinted surface. Goble currently has the fills (`Success`, `Warning`, `Error`) but not the foreground pairs, which is why §2 adds them.

### 1.3 Radius scale

`--radius: 0.5rem` (8px), and every other radius is derived from it:

| Token | CSS | px |
|---|---|---|
| `--radius-sm` | `calc(var(--radius) - 4px)` | 4 |
| `--radius-md` | `calc(var(--radius) - 2px)` | 6 |
| `--radius-lg` | `var(--radius)` | 8 |
| `--radius-xl` | `calc(var(--radius) + 4px)` | 12 |
| `--radius-2xl` | `calc(var(--radius) + 8px)` | 16 |
| `--radius-3xl` | `calc(var(--radius) + 12px)` | 20 |
| `--control-radius` | `0.5rem` | 8 |

What uses them:

- Buttons: `rounded-[var(--control-radius)]` = 8; compact buttons `rounded-md` = 6; micro/tiny 4.
- Inputs and selects: `rounded-lg` = 8; compact `rounded-md` = 6.
- Menus/dropdowns: `rounded-lg` = 8; menu items `rounded-sm` = 4.
- Dialogs: `rounded-2xl` = 16.
- Settings groups and rows: `rounded-xl` = 12.
- Kbd: `rounded` = 4; checkbox `rounded-[.25rem]` = 4; badges `rounded-sm` = 4 (control badges 8).
- Pill buttons and switches: `rounded-full`.

There is no 14px step. Goble's `Radius::Rounded` (14) is outside T3's scale and should not be used by anything specified here.

### 1.4 Spacing scale

T3 uses Tailwind v4's `--spacing: 0.25rem` (4px) multiplier. The steps that actually appear in settings, the composer and the tab strip:

| Utility | rem | px |
|---|---|---|
| `0.5` | 0.125 | 2 |
| `1` | 0.25 | 4 |
| `1.5` | 0.375 | 6 |
| `2` | 0.5 | 8 |
| `2.5` | 0.625 | 10 |
| `3` | 0.75 | 12 |
| `3.5` | 0.875 | 14 |
| `4` | 1 | 16 |
| `6` | 1.5 | 24 |
| `8` | 2 | 32 |
| `12` | 3 | 48 |

Concrete uses: settings page `px-5 pt-6 pb-12` = 20/24/48, with `sm:px-6` = 24; section `space-y-2.5` = 10; group row padding `px-3 sm:px-4` and `py-3` = 16/12; composer `px-4 pt-3.5 pb-2` = 16/14/8; nav row inset `--sidebar-row-content-inset: 0.625rem` = 10; row and control gaps 8 (`gap-2`).

### 1.5 Type scale

The root is 16px by default. T3 adds three sub-`text-xs` steps for dense chrome:

| Class | px | line-height | Use |
|---|---|---|---|
| `text-5xs` | 7 | 1 | provider icon overlays only |
| `text-4xs` | 8 | 1 | provider icon overlays only |
| `text-3xs` | 10 | 14 | image captions, diagnostics tables |
| `text-2xs` | 11 | 16 | metadata, badges, keyboard hints |
| `text-xs` | 12 | 16 | settings descriptions, tooltips, tab labels, composer context |
| `text-sm` | 14 | 20 | settings labels, nav rows, buttons, inputs |
| `text-base` | 16 | 24 | section titles, dialog body default |
| `text-lg` | 18 | 28 | `xl` buttons |
| `text-xl` | 20 | 28 | dialog titles, empty-state titles |
| `text-2xl` | 24 | 32 | hero empty states |
| `text-3xl` | 30 | 36 | hero empty states at `sm` |

Settings uses `text-base font-semibold` for section titles and `text-sm font-medium` for row labels; T3's settings do **not** use 16px body text. The composer prompt size defaults to **14px** (`DEFAULT_PROMPT_FONT_SIZE = 14` in `packages/contracts/src/settings.ts`), while touch inputs are guarded to at least 16px (`--font-size-prompt-touch: max(var(--font-size-prompt, 1rem), 16px)`); the surrounding controls are `text-sm` (14) and context pills are `text-xs`/`text-sm`.

### 1.6 Border widths

| Width | Where |
|---|---|
| 1px | every control outline, card/group border, row separator, divider |
| 2px | button/badge/switch/checkbox focus ring; workspace focus outline; kbd `border-b-2`; the requested goble tab edge |
| 3px | text-input and select focus ring (`ring-[3px]`) |
| 4px | slider thumb focus halo (`inset 0 0 0 1px var(--background), inset 0 0 0 3px var(--ring)`) |

`* { @apply border-border outline-ring/50; }` and `outline-offset: -2px` are global: all outlines sit inside the control, so a clipped/scrollport edge cannot hide them.

### 1.7 Focus ring

The rules, verbatim from the primitives:

- Button: `focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring focus-visible:ring-offset-1 focus-visible:ring-offset-background`.
- Input: wrapper `has-focus-visible:border-ring` + `has-focus-visible:ring-[3px]` + `ring-inset`.
- Select trigger: `focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-inset`; ghost variant uses 2px.
- Checkbox: `focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-ring`.
- Badge: 2px inset ring + 1px offset.
- ScrollArea viewport: 2px inset ring.
- Sidebar/menu row: `focus-visible:ring-2 focus-visible:ring-inset`.
- Workspace area, when it takes F6 focus: `outline: 2px solid var(--ring); outline-offset: -2px`.
- Menu/select/popover popups force `outline-none ring-0`; the **item highlight** (`data-highlighted:bg-accent`) is the focus indicator inside a menu, not a ring.
- Menus and dialogs are `outline-none` because their own focus trap/roving highlight owns the state.
- `:focus-visible` is a browser heuristic: it appears for keyboard focus, not for a mouse press. Rings are `:focus-visible`-only; no component paints a plain `:focus` ring except where a pointer press also warrants it (the composer's selected chip uses `outline: 2px solid var(--ring); outline-offset: -2px`).

Focus colour is always `--ring` = `--primary`. T3 does not use a separate focus hue.

### 1.8 Motion

T3's duration/easing vocabulary is small and repeated:

| Duration | Use |
|---|---|
| 100ms | chevrons, opacity on tiny state changes |
| 140–150ms | hover transitions, switch thumb translate, popup scale/fade, disclosure chevrons |
| 180ms | composer context-label opacity |
| 200ms | dialog/sheet scale + opacity, backdrop fade, accordion height, switch background |
| 250–300ms | attachment strip / drawer entrances |
| 2.0–2.4s | the two duty-cycled activity animations (`status-pulse`, `skeleton`) |

Easing:

- Standard: `ease-out` (`cubic-bezier(0, 0, 0.2, 1)`).
- Sliding surfaces: `--ease-drawer: cubic-bezier(0.32, 0.72, 0, 1)`.
- Dialog open/close: `ease-in-out` over 200ms, `scale(0.98)` → `scale(1)`, opacity 0 → 1; backdrop opacity 200ms.
- Press feedback: `scale(0.97)` on `:active` buttons.
- Dialog stacking: `--nested-dialogs` shifts each nested popup by `translate-y(1.25rem)` and scales by `1 - 0.1 * n`.

Reduced motion is handled per component with `motion-reduce:transition-none` and, for the two global activity animations, with `@media (prefers-reduced-motion: reduce) { animation: none; }` (`live-activity-focus`, `live-activity-focus-counter`). JS-driven scroll (`scrollIntoView`, timeline follow) checks `matchMedia("(prefers-reduced-motion: reduce)")` and falls back to instant.

### 1.9 Contrast approach and thresholds

T3 has two contrast mechanisms.

**1. The user-facing contrast setting**, `appearanceContrast`, range **50–200%**, default 100 (`packages/contracts/src/settings.ts:150-156`). Despite its name, `apps/web/src/appearanceContrast.ts` contains **no contrast ratio**: it only writes three CSS variables. It is not a checker; it re-mixes every text and border:

```ts
root.style.setProperty("--appearance-contrast-base", `${Math.min(contrast, 100)}%`);
root.style.setProperty("--appearance-contrast-boost", `${Math.max(contrast - 100, 0)}%`);
root.style.setProperty("--appearance-contrast-border-boost", `${Math.max(contrast - 100, 0) / 4}%`);
```

`--contrast-foreground` is `color-mix(foreground, background, base)` then `color-mix(..., target, boost)`, where `target` is black in light and white in dark. Borders use `--contrast-border` with the same base and a quarter-strength boost. Above 100% therefore pushes text toward white/black, and below 100% fades it toward the background.

**2. The measured contrast solver** for imported/syntax/custom colours:

| Where | Threshold |
|---|---|
| `apps/web/src/lib/readableSyntaxTheme.ts` | **4.5:1** (`MIN_CONTRAST`) for every syntax colour on the darkest code surface; **6:1** (`FURTHER_MIN_CONTRAST`) for the colours that need more separation |
| `apps/web/src/vscodeThemeImport.ts` | **4.5:1** for text on an imported surface; **1.1:1** to decide two border colours are distinct |
| `apps/web/src/themePalette.ts` | **4.5:1** for `readableThemeForeground`; **5:1** for context-chip accents (`CONTEXT_CHIP_CONTRAST`) |
| Relative-luminance pivot | **0.179** — the luminance at which black and white text have equal contrast |
| Solver | binary search on OKLCH `L` only, 18 iterations, so hue/chroma survive; 8-bit rounded value is re-measured |

These are WCAG 2.x contrast ratios: `(L1 + 0.05) / (L2 + 0.05)`. T3 does **not** globally assert 3:1 for control boundaries; its own `--border` (zinc-200) and `--input` (zinc-300) on white are deliberately subtle (≈1.3:1 and ≈1.5:1). That is a gap goble should not copy (§2.6).

### 1.10 Touch and hit-target minimums

T3's coarse-pointer rule is **44×44 CSS px**, applied as an invisible `::after` on controls:

- Buttons: `pointer-coarse:after:min-h-11 pointer-coarse:after:min-w-11` (44×44).
- Badges that are links/buttons: same 44×44.
- Select trigger: `pointer-coarse:after:min-h-11`.
- Inline buttons: `pointer-coarse:min-h-11 pointer-coarse:px-1.5`.

Mouse-size controls (not touch targets): default button 36px→32px at `sm`; icon 36→32; `icon-lg` 40→36; `icon-sm` 32→28; `xs` 28→24; `compact` 28; `icon-micro` 20; `icon-tiny` 16. Kbd is 20px high.

### 1.11 Keyboard conventions

| Surface | Contract | Where |
|---|---|---|
| Dialog / alert dialog | Base UI focus trap; Tab/Shift+Tab cycle inside; initial focus is the first enabled `[data-slot="dialog-action"]`, else the first tabbable (tests pin Cancel); touch opens focus the popup itself; Escape closes; focus returns to the trigger | `ui/dialog.tsx`, `dialog.browser.test.tsx` |
| Menu | Base UI roving highlight; ArrowUp/Down, Home/End, typeahead, ArrowRight into a submenu, Enter/Space selects, Escape closes; highlight is `data-highlighted:bg-accent` | `ui/menu.tsx` |
| Select | Base UI listbox: arrows/Home/End, typeahead, Enter selects, Escape closes; selected value is exposed as an option | `ui/select.tsx` |
| Settings rail | Up/Down/Home/End move focus between `[data-settings-section]` and scroll the target into view; no roving tabindex — all sections are tabbable | `settingsSectionNavigation.ts` |
| Settings search | `role="combobox"` + `aria-autocomplete="list"` + `aria-activedescendant`; ArrowDown/Up wrap, Enter opens, Escape clears; `/` or Cmd/Ctrl+F focuses the field | `SettingsSidebarNav.tsx` |
| Tabs (right panel, terminal drawer) | **No tablist semantics.** The tab body is a `div` with pointer/drag handlers; the title is a nested button; the close button has `aria-label`. Arrow-key tab navigation is absent. | `RightPanelTabs.tsx:1005`, `ThreadTerminalDrawer.tsx:1400` |
| Composer | Enter submits; Cmd/Ctrl+Enter is the agent gesture; Shift+Enter newline; Shift+Tab cycles approval mode; Escape closes the open command/model band | `ChatComposer.tsx`, `ComposerPromptEditorTiptap.tsx` |
| Escape | Closes the topmost transient surface (dialog → menu → popover), then cancels an edit; it does not close a settings page or a tab | throughout |

T3's menu focus is a roving focus/highlight owned by Base UI (the highlighted item is `data-highlighted`); goble has no DOM focus, so it keeps an explicit highlight index (`PopupMenu::with_highlight_index`) — the same model, spelled in app state.

### 1.12 Reduced motion, zoom and high contrast

- **Reduced motion:** per-component `motion-reduce:transition-none`, plus `prefers-reduced-motion: reduce` blocks for the global animations and JS `matchMedia` checks for smooth scrolling. There is no app-level switch.
- **High contrast:** `appearanceContrast` 50–200% (§1.9), plus a `forced-colors: active` fallback that hands sliders back to the native control (`appearance: auto; accent-color: Highlight`) and disables live-tool shine/ping animations. T3 does not query `prefers-contrast`.
- **Zoom/text size:** the Electron app zooms the main window via menu roles (`CmdOrCtrl+=`, `-`, `0`) and separately exposes interface font size **12–20px** (default 16), prompt size 12–20 (default 14) and code size 10–18 (default 13) via `applyAppearanceFontVariables` (sets `document.documentElement.style.fontSize`). These are two independent mechanisms: page zoom scales everything, font size scales only type.
- **Platform a11y:** Electron/Chromium supplies the accessibility tree, screen-reader support, native high contrast and zoom. Goble has none of that (see §2.1).

### 1.13 Settings architecture, measured

The settings surface is a route-level two-column modal, not a page (`apps/web/src/routes/_chat.settings.tsx`):

| Region | Classes / numbers |
|---|---|
| Rail | `<nav role="navigation">` with `flex w-60 shrink-0 flex-col border-e bg-sidebar text-sidebar-foreground max-md:w-14`; 240px desktop, 56px icon rail below `md` |
| Rail group | `flex flex-col gap-1 max-md:border-t max-md:border-sidebar-border max-md:pt-4`; heading `px-2 text-xs font-medium text-sidebar-muted-foreground/80`; group `role="group" aria-labelledby=...` |
| Rail row | `h-8 rounded-[var(--control-radius)] px-[var(--sidebar-row-content-inset)] py-1.5 text-sm` (10px inset) |
| Rail selected | `data-[active=true]:bg-sidebar-row-selected data-[active=true]:font-medium data-[active=true]:text-sidebar-foreground`; `aria-current="page"` |
| Rail hover/focus | `hover:bg-sidebar-row-hover`; `focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring` |
| Rail search | 32px row (`h-8 rounded-md px-2 py-1.5 text-sm`), `role="combobox"`, `aria-autocomplete="list"`, `aria-expanded`, `aria-controls`, `aria-activedescendant`; Kbd `/` when empty; `role="status"` "No settings found" at `px-2 py-6 text-center text-xs` |
| Content header | `flex h-12 shrink-0 items-center gap-3 border-b ps-5 pe-2` (48px, 20 left / 8 right), breadcrumb |
| Page | `mx-auto flex w-full flex-col gap-8 px-5 pt-6 pb-12 sm:px-6 max-w-4xl` (896px, 32px section gap, 20/24 horizontal, 24 top, 48 bottom) |
| Section | `space-y-2.5` (10px between heading and group); heading wrapper `flex min-h-7 ... px-3 sm:px-4`, `<h2 class="flex min-h-7 items-center gap-2 text-base font-semibold">` |
| Group | `rounded-xl border border-border/60 bg-card/40 shadow-xs/5`; rows divided by `*:not-first:border-t *:not-first:border-border/50`; rows inside get `rounded-none` |
| Row | `rounded-xl px-3 sm:px-4 py-3` (or `pt-3 pb-1` when it has children); `flex ... @min-[32rem]/settings-row:grid @min-[32rem]/settings-row:grid-cols-[minmax(0,1fr)_auto] ... gap-8`, items centre |
| Row text | title `text-sm font-medium` in a `min-h-5` line; description `text-xs leading-normal text-muted-foreground max-w-2xl`, 4px below; status `pt-0.5 text-xs` |
| Row control | right group `flex ... gap-2`; `data-slot=select-trigger` min width 160px; reset/inheritance icons live in fixed `h-5 w-5` slots beside the title |
| Unavailable row | control wrapper `inert` + `opacity-50`; explanation in a tooltip (`inertControl`), row itself `aria-disabled:opacity-64` |

Four representative panels:

1. **Appearance** (`AppearanceSettingsPanel.tsx`): first `SettingsSection id="appearance" title="Colors & themes" variant="plain" hideTitle` holds `ThemeLibrary` unframed; then `SettingsSection id="appearance-interface" title="Interface"` holds rows. The contrast row's control is a `sm:w-52` group: an `<output>` `min-w-12 rounded-md bg-muted px-2 py-1 text-center font-mono text-xs tabular-nums` plus `<input type=range class="settings-slider">`; the row carries a `SettingResetButton` (20px `icon-micro` ghost) only when the value differs from the default. This is the row pattern: label + description left, a bounded control right, an optional reset in the title line.
2. **General** (`SettingsPanels.tsx:791`): `SettingsPageContainer` → `SettingsSection id="notifications" title="Notifications"` with a `NotificationSettings` row (a `SelectTrigger size="sm" class="w-full sm:w-56"`) then an in-app-notifications row with a `Switch aria-label`; `SettingsSection id="sidebar" title="Sidebar"` with a project-grouping `Switch`, a project-order `SelectTrigger size="sm" class="w-full sm:w-44"`, and a working-shelf switch. Every row supplies a `description`; destructive reset is a header action (`Restore device defaults`, `size="xs"` ghost, disabled when nothing changed), not a row.
3. **Archived threads** (`ArchivedThreadsPanel.tsx`): no data is not a blank page — it is one `SettingsRow` whose title is an icon + `No archived threads` (or `Loading archived threads` / `Could not load archived threads`), whose description is `"Archived threads will appear here."` (or the error), and whose control is absent. With data, one `SettingsSection` per project, `title={title}`, `icon={<ProjectFavicon>}`, and each thread row has two `icon-sm` actions (`outline` / `destructive-outline`) with `aria-label={`${label} ${thread.title}`}`, plus a right-click context menu; delete goes through `confirmAndDeleteThread` (an AlertDialog), not a second click.
4. **Connections** (`ConnectionsSettings.tsx`): `FoldedSettingsSection` is the group idiom — a `Collapsible` whose header is `flex flex-wrap items-center gap-x-4 px-3 sm:px-4`; trigger `min-h-11 ... rounded-md`, chevron `size-4 transition-transform duration-150 motion-reduce:transition-none` rotating 90° when open; summary `text-xs text-muted-foreground`; the panel body is a `SettingsGroup` with 1px/50 row rules. Environment-wide rows are `serverScoped`; project overrides are `SettingInheritance` + `SettingResetButton`; unavailable rows use the inert-control tooltip.

### 1.14 Tab strip, measured

T3 has two tab strips — the right-panel surface strip (`apps/web/src/components/RightPanelTabs.tsx`) and the terminal drawer's grouped list (`ThreadTerminalDrawer.tsx:1400`) — and both use the same tab recipe: `h-6 rounded-md pr-2 pl-1.5 text-xs`, active `bg-accent text-foreground`, inactive `text-muted-foreground hover:bg-accent/60`. The table below measures the horizontal right-panel strip; the drawer's rows are the same recipe at `w-full` inside a `flex flex-col gap-0.5`. Neither is a top-level workspace tab bar.

| Property | Value |
|---|---|
| Bar | `flex h-[var(--workspace-topbar-height)] min-h-[var(--workspace-topbar-height)] items-center gap-1 pl-2`; `--workspace-topbar-height: 52px`; desktop titlebar drag region |
| Tab list | `ScrollArea` with `hideScrollbars` + `scrollFade`, inner `flex h-full w-max min-w-full items-center gap-1` |
| Tab | `h-6` (24px), `max-w-36` (144px), `rounded-md` (6px), `pr-2 pl-1.5` (8/6), `text-xs` (12px), `gap-0.5` (2px) |
| Active | `bg-accent text-foreground` — a **fill only**. There is no underline, no dot and no bottom edge. |
| Inactive | `text-muted-foreground hover:bg-accent/60 hover:text-foreground` |
| Indicator | None beyond the active fill. A pending preview tab draws a 6px dot at `-right-0.5 -bottom-0.5`. |
| Close | `PanelTabCloseButton`: `size-4` (16px), `rounded-sm`, hover `bg-muted`; the tab's favicon/icon is `size-3` and is hidden on `group-hover/tab` or `group-focus-visible/close`, replaced by `<X class="size-3">`; `aria-label="Close {title}"`; optional Tooltip. |
| Focus | No designed ring on the tab body or close button; the nested title `<button>` has no ring class and falls back to the browser's default outline. The close button reveals the X on focus. There is **no** `role="tab"`, `role="tablist"`, `aria-selected` or arrow-key navigation. |
| Other | dnd-kit horizontal reorder; middle-click closes; right-click menu (rename/copy/mute/close/close-others/close-to-right/close-all); double-click rename uses a 96px input with `ring-1 ring-inset ring-ring`. |

The active treatment is therefore **a 3–4% fill in dark** (`--accent: white/4%`) plus full-contrast text, not an underline. The 2px `Border`/`Accent` edge in §3.3 is a goble addition the owner asked for, not a T3 parity fix.

### 1.15 Composer, measured

`ChatComposer.tsx` (JSX) + `ComposerSurface.tsx` (frame):

| Part | T3 value |
|---|---|
| Width | `max-w-(--chat-content-max-width)` = `46rem` = 736px, centred; the `.chat-composer-lane` adds 48px inline gutters at ≥640px |
| Frame | `Host` `rounded-2xl` (16px), `p-px` main, `--shadow-composer: 0 1px 2px rgb(0 0 0 / 5%)`, border via a 1px `after:border` in `--chat-composer-outline` (`rgb(0 0 0 / 8%)` light, white 5% dark) |
| Surface | `--chat-composer-glass-surface` + `--glass-opacity: 80%`, `backdrop-blur` (12px light / 16px dark) — a glass card |
| Body padding | `px-4 pb-2` + `pt-3.5 sm:pt-4` = 16 left/right, 14/16 top, 8 bottom |
| Editor | ProseMirror, no box of its own; placeholder `Ask anything. @ files, $ skills, / commands` (`DEFAULT_COMPOSER_PLACEHOLDER`), 14px prompt default; `composer-tiptap:focus { outline: none }`; the caret is the editor focus indicator; a selected chip gets `outline: 2px solid var(--ring); outline-offset: -2px` |
| Footer row | `flex ... px-2 pb-2` (8/8), `gap-2` (8), controls `gap-0.5` (2) |
| Context/model pills | `ComposerControl` `h-7` (28px) at desktop, `sm:h-6` (24px) compact, `px-2`, `text-sm`/`text-xs`, icon 16/18; hover `text-foreground`; menu `data-popup-open` underline for pickers |
| Send | `size-8` (32px) round, `bg-message-action` (`--primary`), `--primary-foreground` glyph, `hover:bg-message-action-hover`, `focus-visible:ring-2 ring-inset`, disabled `opacity-64`; compact text pill `h-8 sm:h-7 px-3/px-4` |
| Stop | 32px `bg-destructive/90` with white glyph (`size-8`), shadow, hover 100% destructive; only while running |
| Attachments | image thumbs `h-16 w-16` (64px) `rounded-lg border border-border/80`; file rows `py-1 text-sm` with a 12px muted size; remove control in the corner |
| Keyboard hints | `Kbd` `h-5` (20px), `min-w-5`, `rounded`, `text-xs`, `bg-foreground/[0.07]` or `border border-b-2 border-foreground/15`; the placeholder suggestion adds `⇥ Tab` |
| Disabled | `disabled:opacity-64`; approval/choice states disable the editor and replace the placeholder ("Choose an option above", "Type your answer", etc.) |
| Streaming | send becomes stop; the composer stays editable; a `role="status" aria-atomic` live region announces suggestion/menu states (not the streamed answer) |
| Mobile | the composer morphs to a compact docked row (`px-3 py-2`, 32px round send) with a view-transition; `--font-size-prompt-touch` clamps the input to ≥16px so iOS does not zoom |

There is **no focus ring on the T3 composer card**; focus is the caret and (for a selected chip) a 2px ring. Goble adds the card ring (§3.2) because a caret alone is not a reliable keyboard focus indicator in a custom renderer.


---

## 2. Mapping T3 → goble

### 2.1 What can and cannot be ported (stated honestly)

Goble is a custom wgpu renderer. It paints quads, glyphs and icons; the OS sees **one surface**. There is no DOM, no accessibility tree, no ARIA, no `role`, no accessible name, no live region, no browser focus traversal, no `:focus-visible`, no `prefers-reduced-motion`, no `forced-colors`, no page zoom and no backdrop blur. This is not a temporary implementation detail; it is the platform.

| T3 mechanism | Goble reality | What this spec does instead |
|---|---|---|
| ARIA roles, names, descriptions, states | Impossible without an accessibility bridge | **Do not claim screen-reader support.** Goble is keyboard- and pointer-operable, not screen-reader-operable. Release notes must say that. Future: an `accesskit` tree is the only real fix, and it is out of scope here. |
| Browser focus order / `:focus-visible` | No DOM; the app owns focus in `UiState` | Explicit focus state per region + a **visible ring drawn by the element**, with a keyboard-modality flag so a mouse press does not paint a ring. |
| Base UI focus trap / roving tabindex / typeahead | No library; menus already carry a highlight index | Re-implement the small contract: trap while a modal is up, restore the opener, roving highlight in menus, explicit `aria`-equivalent state names in the element (for tests), not for the OS. |
| `prefers-reduced-motion`, `forced-colors` | Media queries do not exist in wgpu | A `Motion`/`Contrast` value on `Theme`, read from a platform API once at startup and from a setting. |
| Page zoom (Cmd +/-) | No page; winit reports a scale factor, but there is no CSS zoom | A text-scale multiplier on the type tokens (12–20) plus the OS scale factor. Window zoom is not a substitute. |
| Backdrop blur / glass | `backdrop-filter` has no wgpu equivalent at this layer | Opaque surface mixes (`Surface`, `SurfaceMuted`, `Scrim`) instead of blur. |
| CSS `color-mix` / alpha | `ColorU` supports alpha, but `mix` drops it | Add `ColorU::mix_over` / `contrast_ratio`; pre-mix translucent tokens to opaque where the renderer has no blend, or pass alpha explicitly. |
| Text selection / clipboard / IME | Goble has custom caret, selection and clipboard code; no OS text interface | Keep it; it is not an accessibility feature and must not be described as one. |
| Native context menu / focus rings | Goble draws its own | Port the **shape** (2px, inset, ring colour), not the CSS mechanism. |

The hard sentence for the README/release note:

> Goble's interface is drawn with wgpu, so it does not expose an accessibility tree. VoiceOver and other screen readers cannot see or operate its controls. Goble compensates with full keyboard reachability, visible focus, 4.5:1 text contrast, 3:1 control/focus contrast, 44px touch targets where a pointer is coarse, and a reduced-motion mode — but it is not a screen-reader-accessible application today.

Everything below is chosen so that sentence stays true and the compensation is real.

### 2.2 New goble tokens

Names are proposed; none exist in `crates/goble-ui/src/theme.rs` today.

**Colour (`ColorToken`)**

| New token | Value / resolution | Why |
|---|---|---|
| `AccentForeground` | Dynamic: `#fff` if `contrast_ratio(white, Accent) >= contrast_ratio(black, Accent)`, else `#000`; for the default neutral accent this resolves to **black** (white on `#9a9a9a` is 2.81:1) | T3's `--primary-foreground`; goble's accent varies per preset, so this cannot be a static colour |
| `Placeholder` | Light `#696969`, dark `#8e8e8e` | T3 separates placeholder from body text; goble currently uses `Muted` for both |
| `InputBorder` | Light `#8a8a8a` (3.03:1 on `#f0f0f0`, 3.45:1 on white), dark `#6b6b6b` (3.13:1 on `#1e1e1e`, 3.62:1 on `#0e0e0e`) | T3's `--input`; goble's `Border` is 1.28–1.32:1 and is fine for separators but not for the only boundary of a control |
| `BorderStrong` | Light `#6b6b6b`, dark `#8e8e8e` | T3's boosted `--contrast-border`; used by the high-contrast mode |
| `SurfaceMuted` | `Surface.mix(Hover, 0.5)` → `#f8f8f8` light, `#1a1a1a` dark | T3's static `--muted` fill; distinct from `Hover`, which must stay a state |
| `Scrim` | `Bg` at alpha 153 (60%); no blur | T3's `--background/60%` dialog backdrop; goble has no blur, so the alpha is the whole effect |
| `Info` | Light `#3b82f6`, dark `#60a5fa` | T3's `--info`; harness status rows need a fourth semantic colour |
| `InfoForeground` | Light `#1d4ed8`, dark `#93c5fd` | Text on info tints |
| `SuccessForeground` | Light `#047857`, dark `#34d399` | Text on success tints / 8% surfaces |
| `WarningForeground` | Light `#b45309`, dark `#fbbf24` | Text on warning tints |
| `ErrorForeground` | Light `#b91c1c`, dark `#f87171` | T3's `--destructive-foreground`; the fill stays `Error` |

**Spacing (`SpacingToken`)**

| New token | px | T3 source |
|---|---|---|
| `Xxs` | 2 | `0.5` |
| `Xxl` | 32 | `8` |
| `Xxxl` | 48 | `12` |

Keep `Xs=4`, `Sm=8`, `Md=12`, `Lg=16`, `Xl=24`. T3's 6/10/14 are rounded to 8/12/16 on goble's 4px grid; that is deliberate, not an oversight (§5 Q6).

**Radius**

Introduce `RadiusToken::{Sm, Md, Lg, Xl, Xxl}` = **4, 6, 8, 12, 16**, resolved by `Theme::radius_token(RadiusToken)`. Keep the three `Radius` presets for compatibility: `Sharp`→0, `Default`→`Lg` (8), `Rounded`→14 (legacy, not used by anything in this spec). `--control-radius` maps to `Lg` (8); settings groups to `Xl` (12); composer/dialog to `Xxl` (16).

**Type**

Introduce `TypeToken::{Micro, Caption, Body, Label, Title, Heading, Display}` = **10, 11, 12, 14, 16, 20, 24**, multiplied by `Theme::text_scale` (default 1.0; range 0.75–1.25 for the 12–20px interface-size setting). Deprecate the bare `13.0` used in `app/src/ui/settings.rs`: T3 has no 13px step; settings labels become `Label` (14), descriptions `Body` (12), section titles `Title` (16).

**Focus / borders / motion**

| Token | Value |
|---|---|
| `FocusStyle::control` | width **2px**, offset **−2px** (inside), colour `Focus` |
| `FocusStyle::field` | width **3px**, offset **−3px** (inside), colour `Focus`, border becomes `Focus` |
| `FocusStyle::tab_edge` | width **2px**, full tab width, colour `Border` at rest / `Accent` selected |
| `Theme::border_width` | 1.0 |
| `Motion::fast_ms` | 100 |
| `Motion::base_ms` | 150 |
| `Motion::slow_ms` | 200 |
| `Motion::drawer_ms` | 300 |
| `Motion::ease_out` | `cubic-bezier(0.0, 0.0, 0.2, 1.0)` |
| `Motion::ease_drawer` | `cubic-bezier(0.32, 0.72, 0.0, 1.0)` |
| `Motion::reduced` | false; when true, every duration above returns 0 |
| `Contrast::Normal` / `High` | `High` substitutes `BorderStrong`, raises `Muted` to `Text` and focus width to 3px |

Add `ColorU::relative_luminance()`, `ColorU::contrast_ratio(other)`, `ColorU::mix_over(base, alpha)` in `crates/goble-ui/src/color.rs` so every token test uses the same WCAG formula as §1.9.

### 2.3 Colour mapping table

`action` is one of **keep** (already right), **adapt** (same token, new value/shape), **new** (token added above), **reject** (not ported).

| T3 | Goble | Action | Rule |
|---|---|---|---|
| `--background` | `Bg` | keep | Canvas |
| `--foreground` | `Text` | keep | Body text |
| `--card` | `Surface` | keep | Panels/cards |
| `--popover` | `SurfaceRaised` | keep | Menus/dialogs/composer controls |
| `--primary` | `Accent` | keep | Goble's accent stays neutral/preset-owned; do **not** import T3's blue |
| `--primary-foreground` | `AccentForeground` | **new** | Dynamic contrast decision (§2.2) |
| `--secondary` / `--muted` | `SurfaceMuted` | **new** | Static quiet fill |
| `--muted-foreground` | `Muted` | adapt | Light value `#6f6f6f` → `#696969` so it clears 4.5:1 on `SurfaceRaised` (4.41 → 4.82) |
| `--placeholder` | `Placeholder` | **new** | Same values as adjusted `Muted` initially |
| `--accent` | `Hover` | keep | Pointer/keyboard highlight |
| `--accent-foreground` | `Text` | keep | Highlight label |
| `--border` | `Border` | keep | 1px rules |
| `--input` | `InputBorder` | **new** | Only boundary of a field/select; 3:1 |
| `--ring` | `Focus` | adapt | Light `#00c2ff` (2.07:1 on white) → `#007a9e` (4.91:1 on white, 4.31:1 on `#f0f0f0`); dark stays `#19aad8` (7.15:1 on `Bg`) |
| `--destructive` | `Error` | keep | Stop/delete fill |
| `--destructive-foreground` | `ErrorForeground` | **new** | Text on error tints |
| `--success` | `Success` | keep | Status fill |
| `--success-foreground` | `SuccessForeground` | **new** | Status text |
| `--warning` | `Warning` | keep | Status fill |
| `--warning-foreground` | `WarningForeground` | **new** | Status text |
| `--info` / `--info-foreground` | `Info` / `InfoForeground` | **new** | Harness status |
| `--sidebar` | `Surface` | keep | Rail |
| `--sidebar-row-hover` | `Hover` | keep | Rail hover |
| `--sidebar-row-selected` | `Selected` | keep | Rail selected |
| `--sidebar-border` | `Border` | keep | Rail rule |
| `--code-background` | `SurfaceRaised` | keep | Code block |
| `--message-surface` | `SurfaceRaised` | keep | User turn |
| `--message-action` | `Accent` | keep | Send |
| `--diff-addition` / `--diff-deletion` | `DiffInsertBg` / `DiffDeleteBg` | keep | Diffs |
| boosted `--contrast-border` | `BorderStrong` | **new** | High contrast |
| `--background` at 60% + blur (dialog backdrop) | `Scrim` | **new** | Alpha 60%, no blur |
| `--selection` band | `Selection` | keep | Accent mixed 55% toward `Bg` |

Rejected: `--glass-*`, `--surface-grain`, `backdrop-filter` shadows/blur, stage artwork tokens, the 7px/8px `text-4xs`/`5xs` overlays. These have no wgpu analog or no goble consumer.

### 2.4 Spacing / radius / type / border mapping

| T3 | Goble | Action |
|---|---|---|
| `0.5` (2) | `SpacingToken::Xxs` | new |
| `1` (4) | `Xs` | keep |
| `1.5` (6) | `Sm` (8) | adapt: round up |
| `2` (8) | `Sm` | keep |
| `2.5` (10) | `Md` (12) | adapt: round up |
| `3` (12) | `Md` | keep |
| `3.5` (14) | `Lg` (16) | adapt: round up |
| `4` (16) | `Lg` | keep |
| `6` (24) | `Xl` | keep |
| `8` (32) | `Xxl` | new |
| `12` (48) | `Xxxl` | new |
| `--radius-sm` 4 | `RadiusToken::Sm` | new |
| `--radius-md` 6 | `RadiusToken::Md` | new |
| `--radius-lg` 8 | `RadiusToken::Lg` | new (= `Radius::Default`) |
| `--radius-xl` 12 | `RadiusToken::Xl` | new |
| `--radius-2xl` 16 | `RadiusToken::Xxl` | new |
| `--radius-3xl` 20 | — | reject (nothing in goble needs it) |
| `text-3xs` 10 | `TypeToken::Micro` | new |
| `text-2xs` 11 | `TypeToken::Caption` | new |
| `text-xs` 12 | `TypeToken::Body` | new |
| `text-sm` 14 | `TypeToken::Label` | new |
| `text-base` 16 | `TypeToken::Title` | new |
| `text-xl` 20 | `TypeToken::Heading` | new |
| `text-2xl` 24 | `TypeToken::Display` | new |
| 1px | `Theme::border_width` | keep |
| 2px focus | `FocusStyle::control` | new |
| 3px field focus | `FocusStyle::field` | new |
| 2px tab edge | `FocusStyle::tab_edge` | new |
| 100/150/200/300ms | `Motion::*` | new |
| `ease-out` | `Motion::ease_out` | new |
| `--ease-drawer` | `Motion::ease_drawer` | new |

### 2.5 Adopt / keep / reject, explicitly

**Adopt:** 8px control radius, 12px group radius, 16px dialog/composer radius, 4/6/8/12/16 radius steps; 4/8/12/16/24 spacing with 2/32/48 added; 14px settings labels and 12px descriptions; 16px section titles; 2px inset focus ring (3px for fields); 1px separators; 2px tab edge; 4.5:1 text and 3:1 control/focus contrast; 44px coarse-pointer targets; 200ms surface transitions with `ease-out`; reduced-motion = no animation.

**Keep:** goble's neutral, preset-owned accent (not T3 blue); goble's dark canvas `#0e0e0e` (not T3's `#000`); goble's 12px dense body (T3's 14px settings label is adopted, but the app's default text stays 12 for density); goble's three `Radius` presets for compatibility; goble's full-height square workspace tabs and their colour tint; goble's row-level keyboard model (a settings **row** owns focus, so the ring wraps the row, not a DOM control).

**Reject:** glass/backdrop blur, surface grain, `prefers-contrast`/`forced-colors` as mechanisms, browser page zoom, T3's Radix/Base UI dependency, T3's `--radius-3xl` 20, T3's 6/10/14 half-steps, T3's translucent borders without a blend model.

---

## 3. Concrete redesign specs

Each subsection is sized to be one verified implementation item (§4), with exact numbers, the owning file, and the test to write in the repo's existing drawn-command style. "Frame" below means: layout at a known window, paint into a `Renderer`, inspect `RenderCommand`s — the pattern in `app/src/ui/settings_category_tests.rs` (`frame`, `drawn_text`, `bg_fills`) and `app/src/ui/shell/tests.rs` (`render_tab`, `paint_with_pointer`).

### 3.1 Settings pane (rail + page + row)

**Current, measured** (`app/src/ui/settings.rs`):

| Piece | Current |
|---|---|
| Rail | `NAV_WIDTH = 148.0`, `RULE_WIDTH = 1.0`, `Surface` fill, 8px uniform padding, a 1px `Divider` between every pair of rows |
| Rail row | 12px label, 8px uniform padding, `HoverRow` with `Selected` fill when current; focus = 1px `Focus` border, radius 8 |
| Header | 13px "Settings", 24px close, padding `sm`/`md`, `FOCUS_MARKER_SIZE` inset when active |
| Page | `md` (12px) uniform padding around a `Scrollable`; section headings 11px `Muted` with 8/12/4 padding; flat `Divider` between every row |
| Row | 12px label, 8px uniform padding, control right; no description; focus = 1px `Focus` border around the whole row |
| Footer | `ShortcutHints`, 8/12 padding, 1px `Divider` above |

**Target (T3-shaped, goble-drawn).**

| Element | Value | Notes |
|---|---|---|
| Rail width | **200px** | T3 is `w-60` (240) with icons; goble has no settings icons, so 200 holds a 14px label plus 10px insets. `NAV_WIDTH` becomes 200, `RULE_WIDTH` stays 1. |
| Rail background | `Surface` | keep |
| Rail padding | **8px** all sides | `Sm` |
| Rail rows | height **32px**, horizontal inset **10px**, radius **8px**, label **14px/400** (`Label`), selected **14px/500** | T3: `h-8 rounded-[var(--control-radius)] px-[var(--sidebar-row-content-inset)] py-1.5 text-sm` |
| Rail row gap | **2px** (`Xxs`), **no dividers** | T3 uses `SidebarMenu` `gap-1` = 4px plus no rules; 2px suits goble's 32px rows |
| Rail selected | fill `Selected`, text `Text` | current selected uses `HoverRow`; keep the element, change its geometry |
| Rail hover | fill `Hover` | |
| Rail focus | **2px `Focus` ring**, inset 2px, radius 10 (row radius + 2) | row-level focus, because goble focuses rows not controls |
| Page padding | **24px top/right/bottom, 24px left** (`Xl`), max content width **896px** | T3: `max-w-4xl px-5 pt-6 pb-12 sm:px-6 gap-8` |
| Section gap | **32px** (`Xxl`); header → group **10px** | T3: `gap-8` (`SettingsPageContainer` overrides the container's `gap-6`), `space-y-2.5` |
| Section header | **16px/600 `Text`**, min-height **28px**, horizontal 16px | T3: `text-base font-semibold min-h-7 px-3 sm:px-4` |
| Group card | radius **12px**, fill `Surface` at **40% alpha** (or `Surface.mix(Bg, 0.6)`), border **1px `Border`/60**, **no shadow** | T3: `rounded-xl border border-border/60 bg-card/40 shadow-xs/5`; goble has no blur shadow, so the border does the work |
| Row separator | **1px `Border`/50** between rows, none above the first/below the last | T3: `*:not-first:border-t *:not-first:border-border/50` |
| Row padding | horizontal **16px**, vertical **12px**; with a child editor: top 12, bottom 4 | T3: `px-3 sm:px-4 py-3` / `pt-3 pb-1` |
| Row layout | label column `1fr`, control `auto`, gap **32px** (`Xxl`), centred when ≥ 512px; stacked below | T3: `grid-cols-[minmax(0,1fr)_auto] gap-8` at `@min-[32rem]` |
| Row label | **14px/500 `Text`**, line box **20px** | T3: `text-sm font-medium`, `min-h-5` |
| Row description | **12px/400 `Muted`**, line-height **18px**, max width **672px** (2xl), 4px below the label | T3: `text-xs leading-normal max-w-2xl`, `space-y-1` |
| Row height | **44px** minimum with no description; **66px** with one 18px description line (12 + 20 + 4 + 18 + 12) | measured from T3's box model |
| Control | right-aligned, gap 8px; select min width **160px** | T3: `gap-2`, `[&_[data-slot=select-trigger]]:min-w-40` |
| Row focus | **2px `Focus` ring**, inset 2px, radius 12; drawn around the whole row | goble's focus is the row; this is the honest adaptation |
| Row hover | none for a setting row; `Hover` for a row that itself activates | T3 setting rows are not clickable |
| Header band | **48px** high, bottom border **1px `Border`/60**, horizontal padding **20px**, title **14px/600** | T3 settings header is `h-12 border-b ps-5 pe-2` |
| Close | **24×24** (`TopbarButton`), 16px glyph, `Muted`, hover `Hover` | keep |

**States.**

- Selected rail row: `Selected` fill + `Text` + 500 weight. Hover on selected: `Hover` (still selected text).
- Focused rail row: 2px `Focus`; when the row is also selected, the ring is drawn **over** the selected fill, not replacing it.
- Focused page row: 2px `Focus` ring around the row, and the control keeps its own focus treatment when the caret is in a field. Only one 2px ring is visible: a focused row's ring and a focused field's 3px ring must not both be painted on the same row (the field's ring wins).
- Disabled/unavailable: row content at **64% opacity** (`T3 data-disabled:opacity-64`), control inert, explanation in the description line — not a tooltip-only message.
- Search target (jump from search): 2px `Focus` ring plus a 600ms `Selection`-based pulse; with `Motion::reduced`, the pulse is skipped.

**Copy rules (the "friendlier" half).**

- Every row gets a one-sentence description unless its label is already a complete action ("Reload models"). The description says what changes, not which widget it is: **"Combine matching repositories across environments."** not "Toggle project grouping."
- Sentence case, ends with a period, no exclamation mark, no "Please". T3's own examples are the register: **"Adjust the contrast of colors and borders across the interface."**, **"Higher values make menus, dialogs, and the composer more solid."**, **"Order of projects in the sidebar project picker and command palette."**
- One sentence, at most two lines at the 672px description width. If it needs a second sentence, it is a folded section (§1.13, `FoldedSettingsSection`) or a tooltip on a `?`, not a paragraph in the row.
- Destructive rows name the object and whether it can be undone: "Delete this group and its secrets. This cannot be undone." The confirm button repeats the verb, not "OK".
- Unavailable rows explain the condition in the description ("Managed by Acme.") rather than only in a hover tooltip, because a keyboard user may never hover.

**Search affordance.**

T3 puts search in the rail, above the list: a 32px row, `Search` placeholder, `/` Kbd when empty, `role="combobox"` with `aria-activedescendant`, a listbox of results, "No settings found" in 12px `Muted`, and Escape clears. Goble has `SearchInput` already (`crates/goble-ui/src/elements/search_input.rs`, `Surface` fill, 1px `Focus` border when focused, `Muted` placeholder). Add it to the rail with:

- height **32px**, radius 8, 10px horizontal inset, 12px text: `SearchInput::with_compact(true)` (4px vertical padding) + `with_extra_height(4.8)` gives 32px at a 12px font.
- When the query is empty: a `Kbd`-sized `/` cap at the right, 20×20, radius 4, 11px.
- When non-empty: a 20×20 clear `IconButton` (`x`, 10px glyph), `Muted`.
- Results: max 8 rows, 44px each, label 14/500, section name 11 `Muted` on a second line; Up/Down wrap, Enter jumps, Escape clears and returns focus to the field; the current result fills `Hover` and has a 2px `Focus` ring only when the field is not the focus owner (the field's caret is the indicator, and the list highlight is the secondary indicator).
- No results: centred 12px `Muted` text at 24px vertical padding; no card, no border.

**Owner files.** `app/src/ui/settings.rs` (`NAV_WIDTH`, `build_nav`, `build_pane`, `heading`, `setting_row`, `field_row`, `build_footer`), `app/src/ui/types.rs` (`SettingsControl` descriptions), `app/src/ui/settings_category_tests.rs`. Shared `SettingsRow` candidate: `crates/goble-ui/src/views/settings_view/row.rs` already has a 12px label + description + control row with `ROW_HEADER_PADDING = 15.0` and 12px description. Either lift it into `goble-ui/src/elements/settings_row.rs` and use it from `app/src/ui/settings.rs`, or keep the app builder and copy the geometry. The spec prefers the lift (D4) so the shared row cannot drift from the harness row.

**Acceptance tests.**

1. `a_settings_rail_row_is_32px_with_a_14px_label_and_a_2px_focus_ring`: frame the Appearance page; find the rail row for `Mouse`; assert the `FillRect` for its selected/hover band is 32 high (or the row's layout height is 32), the `DrawText` run for "Mouse" has `font_size == 14.0`, and when the rail owns the keyboard the row is wrapped by a `StrokeRect { width: 2.0, color: Focus }`.
2. `a_settings_row_is_a_12px_card_with_a_14px_label_and_a_12px_description`: assert the group `StrokeRect { width: 1.0, corner_radius: 12.0 }`; assert the row's title run `font_size == 14.0`, description `font_size == 12.0`, and the vertical distance from the row top to the title baseline matches the 12px padding.
3. `a_settings_group_has_one_rule_between_rows`: count `StrokeRect`/divider commands inside the group: `n - 1` rules for `n` rows.
4. `the_settings_header_is_48px_with_one_bottom_rule`: assert the header band's layout height is 48 and exactly one 1px `StrokeRect` sits on its bottom edge.

Verification: `cargo test -p goble-app --lib ui::settings_category_tests`.

### 3.2 Composer

**Current, measured** (`crates/goble-ui/src/elements/chat_composer/composer.rs`):

- `into_card`: padding `md` horizontal (12), `xs` vertical (4), radius `COMPOSER_CONTROL_RADIUS` (4), **no background and no border**. Comment says the raised background and 1px border were deliberately dropped.
- Textarea: min height 48, max 160, `TEXT_FONT_SIZE = 12.0`, no container focus ring; focus is the caret beam.
- Context/action row: `Wrap`, gap 8, pills `CONTEXT_PILL_HEIGHT = 24`, `CONTROL_TEXT_SIZE = 11`, radius 4.
- No send button: Enter submits; Stop is a 24px `ComposerButton` with an `Error` stop icon.
- Attachments: `Chip` per attachment; no fixed height/radius.
- Disabled: empty editor, placeholder "Waiting for your decision…"; `dispatch_event` returns false.
- `edge_to_edge` drops the horizontal card padding and the `xs` outer margin.

**Target.**

| Part | Value | Token / owner |
|---|---|---|
| Max width | **736px** (`46rem`) | `--chat-content-max-width`; app passes a max width or composer clamps |
| Card radius | **16px** | `RadiusToken::Xxl` |
| Card fill | `Surface` (opaque; no glass) | `ColorToken::Surface` |
| Card border | **1px `Border`/60** at rest; the 1px border stays and a **2px `Focus` ring** is added inside when the editor is focused | `StrokeRect`, widths 1 and 2 |
| Padding | left/right **16px**, top **12px**, bottom **8px** | `Lg`, `Md`, `Sm` |
| Editor gap | **8px** between editor and footer row | `Sm` |
| Editor | min height **48px**, max **160px**, text **14px** (`Label`), line-height **1.4** (19.6), placeholder `Placeholder` | `TextArea` |
| Caret | 2px wide `Focus`, height **18px** at 14px text; no separate ring around the editor | existing `caret_beam` |
| Card focus | **2px `Focus` ring** inset 2px, radius 16, when the editor or any footer control is focused; the focused control also gets its own 2px ring only when it is not the editor | `FocusStyle::control` |
| Footer row | min height **28px**, gap **8px**, horizontal 0 (the card padding is the inset) | |
| Context/model pills | height **24px**, radius **6px**, horizontal padding **8px**, text **12px**, icon **14px**, 1px `Border` at rest, `Hover` fill on hover, `Selected` when open | `ComposerButton` / `ContextPill` |
| Send | **32×32** circle, fill `Accent`, glyph `AccentForeground` (black for the default neutral accent), hover `Accent` at 90%, pressed 97% scale, disabled **64%** opacity | new `ComposerButton` variant or `IconButton` |
| Stop | **32×32** circle, fill `Error`, glyph `ErrorForeground`, visible only while a turn streams; no disabled state | `ComposerButton` |
| Attachments | chips **24px** high, radius **6px**, fill `SurfaceMuted`, border 1px `Border`/60, text 12, remove **16×16**, gap 8; image thumbs **64×64**, radius **8** | `Chip` / `ComposerAttachmentStrip` |
| Keyboard hints | `ShortcutHints` caps **20px**, radius **4**, text **11px**, fill `SurfaceMuted`, 1px `Border` | existing caps |
| Disabled | whole card **64%** opacity; placeholder "Waiting for your decision…"; no focus ring, no pointer events | |
| Waiting | keeps the draft and Enter; placeholder "Message the agent while it waits…" | |
| Streaming | editor enabled; Stop visible; send hidden | |
| Narrow (< 420px pane) | padding **12/12/8**, send **28px**, pills wrap to extra rows (the card grows); do not clip or horizontal-scroll; labels stay visible | goble is desktop-only; this is the narrow-pane variant, not a mobile app |

Hit targets: 32px send/stop and 20px close are the drawn mouse sizes; D2 supplies the 44×44 coarse-pointer hit box without changing the glyph. The 32px settings rail row and 24px settings close follow the same rule.

**Why the container ring when T3 has none.** T3's composer relies on the ProseMirror caret and has no ring on the card. Goble's caret is a thin 2px beam on a 1px border; at low contrast that is not a reliable focus indicator. The 2px `Focus` ring is a deliberate **exceeds-T3** choice for accessibility, not a claim that T3 does it.

**Owner files.** `crates/goble-ui/src/elements/chat_composer/composer.rs`, `crates/goble-ui/src/elements/composer_button.rs`, `crates/goble-ui/src/elements/context_pill.rs`, `crates/goble-ui/src/elements/chip.rs`, `crates/goble-ui/src/elements/text_area.rs`, `app/src/ui/chat/composer.rs`.

**Acceptance tests.**

1. `the_composer_card_is_16px_with_a_1px_border_and_16_12_8_padding`: paint the composer in a 600×400 box; find the card `StrokeRect` and assert `width == 1.0`, `corner_radius == 16.0`; measure the text origin minus card origin on x = 16, y = 12; measure the footer bottom = card bottom − 8.
2. `a_focused_composer_draws_a_2px_focus_ring_inside_the_card`: `with_focused(true)`; assert exactly one `StrokeRect { width: 2.0, color: Focus, corner_radius: 16.0 }`, and its rect is inset 2px from the card.
3. `the_send_button_is_32px_and_reads_on_the_accent`: assert `FillRect` 32×32 with `Accent`, glyph colour `AccentForeground`, and `contrast_ratio(AccentForeground, Accent) >= 4.5`.
4. `the_stop_button_is_32px_while_streaming`: `with_stop_visible(true)`; assert a 32×32 `Error` fill and no send control.
5. `a_disabled_composer_draws_the_waiting_placeholder_at_64_percent`: assert the whole card's colours are pre-mixed at 0.64 or the alpha is 163, and the placeholder text is the waiting string.
6. `a_narrow_composer_wraps_pills_instead_of_clipping`: at 360px width, assert the footer's pill rows have more than one `y`, and no pill's right edge exceeds the card's right edge.

Verification: `cargo test -p goble-ui --lib elements::chat_composer::tests`. Existing tests that count `StrokeRect` as "cards" (e.g. the pill test at `chat_composer/tests.rs:321`, the SSH chip test at `:567`) will need their counts updated because the card now strokes a border; that update is part of D6, not a separate item.

### 3.3 Tab strip

**Current, measured** (`app/src/ui/shell/topbar.rs`):

- `TOPBAR_HEIGHT` = 36 (macOS) / 40 (other); every tab is full height (`WorkspaceChip::height()`).
- Tab fill: active `SurfaceRaised`; hover `Hover`; coloured tabs tint at 60/40/20 (`TabTint::Active/Hovered/Resting`).
- Label 12px, active `Text`/Medium, inactive `Muted`/Regular; `h_pad = 7.0`, `close_gap = 4.0`, `MAX_TAB_WIDTH = 200`; 1px `TAB_RULE_WIDTH` between tabs.
- Close: `TopbarButton` 16×16 with a 10px `x`, drawn only while hovered; it shares the trailing slot with the shortcut hint.
- No focus ring; tabs are pointer-only; keyboard tab switching is `Ctrl+Tab`/`Ctrl+Shift+Tab` at the root.
- Existing test: `a_workspace_tab_fills_the_bar_with_square_corners` asserts the tab is full height, square-cornered, and has no border.

**Target (the owner's 2px edge).**

| Property | Value |
|---|---|
| Height | unchanged: `TOPBAR_HEIGHT` 36/40; the content box is `height - 2` and the label is centred in it, so the label moves up 1px |
| Bottom edge | `FillRect` at `x = tab.min_x()`, `y = tab.max_y() - 2`, `w = tab.width()`, `h = 2` |
| Edge colour, rest | `Border` |
| Edge colour, selected | `Accent` |
| Edge colour, hover | `Border` (hover is carried by the fill, not by the edge) |
| Radius | **0**. Tabs stay square; the edge meets both corners squarely. Do not round the tab in this item. |
| Horizontal padding | `h_pad` **8** (from 7); `close_gap` **4** unchanged |
| Close target | **20×20** (from 16), glyph **10**, `Muted`, hover `Hover`, 1px radius 4 hover fill; tooltip unchanged |
| Label | 12px (`Body`), active `Text`/Medium, inactive `Muted`/Regular |
| Focus (D9) | **2px `Focus` ring**, inset 2px, radius 0, drawn around the tab when it owns keyboard focus; the 2px bottom edge stays visible beneath it |
| Tint interaction | The edge is painted **after** the fill and **before** the label/close. A coloured tab keeps its 60/40/20 tint; the edge is `Border` or `Accent`, never the tab colour (Q2) |
| Paint order | fill → bottom edge → close (if hovered) → label; all clipped to the tab |
| Selected fill | `SurfaceRaised` (keep); `Accent` edge is the new selected marker, so selected is no longer fill-only |
| Hover fill | `Hover` (keep); the edge stays `Border` |
| Rule between tabs | keep the 1px `TAB_RULE_WIDTH`; the bottom edge does not replace it |

The 20px close is the drawn mouse target; D2 supplies the 44×44 coarse-pointer box around it.

**Radius interaction, stated once:** goble's tab is square by construction (`fill_rect`, no `fill_rounded_rect`), and the existing test pins that. A 2px bottom edge on a square tab is a full-width rectangle. If a future change rounds tabs to `RadiusToken::Md` (6), the edge must be shortened by 6px at each end (or drawn as a rounded 2px stroke) and `a_workspace_tab_fills_the_bar_with_square_corners` must be replaced, not silently weakened.

**Contrast gate.** The owner asks for `Accent` on selected. In the **light default**, `Accent` `#9a9a9a` against `SurfaceRaised` `#f0f0f0` is **2.47:1**, below the 3:1 non-text minimum. D8 must assert `contrast_ratio(Accent, SurfaceRaised) >= 3.0` for the selected edge; if the owner keeps `#9a9a9a`, the selected edge falls back to `Text` (15.1:1) until the light accent is darkened (Q1/Q2). In dark, `#9a9a9a` on `#1e1e1e` is 5.92:1, so no fallback is needed there.

**Owner files.** `app/src/ui/shell/topbar.rs` (`WorkspaceChip::paint`, `h_pad`, close construction, `TAB_FONT_SIZE`), `app/src/ui/shell/tests.rs`.

**Acceptance tests.**

1. `a_resting_workspace_tab_has_a_2px_border_edge`: render an inactive tab at 200×40; assert a `FillRect` with height 2, width 200, `y = 38`, colour `Border`.
2. `the_selected_workspace_tab_edge_is_accent`: render active; assert the same rect is `Accent` (or `Text` when the contrast gate falls back) and that `contrast_ratio(edge, SurfaceRaised) >= 3.0`.
3. `the_tab_edge_does_not_eat_the_label`: assert the label's centre `y` is `(height - 2) / 2`, and the label's bottom is at least 2px above the edge.
4. `a_tinted_tab_keeps_its_tint_and_its_edge`: active `TabColor::Red`; assert the fill is `red.tint_over(SurfaceRaised, 0.6)` and the edge is still `Accent`/`Text`, not red.
5. `the_close_target_is_20px_and_sits_inside_the_tab`: with the pointer over the tab, assert the close hit box is 20×20 and its right edge is 8px from the tab's right edge; clicking at its centre closes, clicking the label selects.
6. Existing `a_workspace_tab_fills_the_bar_with_square_corners` still passes (height and radius unchanged).

Verification: `cargo test -p goble-app --lib ui::shell::tests`.

### 3.4 Harness window chrome

**Current, measured** (`app/src/ui/harness.rs`):

- `build_page`: `Surface` background, `md` (12px) uniform padding, header row of Back ghost button + 12px title + 11px count, 1px `Divider`, `Scrollable` body with `sm` (8px) spacing.
- Empty state: a single 12px `Muted` `Text` at the top of the body.
- `card`: `SurfaceRaised` fill, `sm` (8px) uniform padding, no radius, no border; each row is its own card, so a list is a stack of 8px-inset blocks.
- Rows: 12px title, 11px metadata, 8px status dot, right-aligned 11px status.

**Target (same standard as settings).**

| Part | Value |
|---|---|
| Page background | **`Bg`** for the page body, header and scroll area; `Surface` is used by the group card, not the whole page |
| Header band | height **48px**, horizontal padding **16px**, bottom border **1px `Border`/60** |
| Back | `ButtonVariant::Ghost`, height **28px**, 13px `arrow-left`, label `Back` 12px; 8px gap to title |
| Title | **14px/600 `Text`** |
| Count | **11px `Muted`**, right-aligned, no badge; hide when empty (current behaviour) |
| Content | max width **896px**, centred; padding **20px** horizontal, **24px** top, **48px** bottom; section gap **24px** |
| List group | radius **12px**, fill `Surface` at 40%, border **1px `Border`/60**; rows separated by **1px `Border`/50** |
| Row | min height **48px** (one line) / **64px** (title + metadata); padding **12px vertical, 16px horizontal**; title **14px/500**; metadata **12px `Muted`**; status **11px `Muted`**; dot **8px**; right inset 16px |
| Row hover | `Hover` fill only for rows that activate; observational rows have no hover |
| Empty state | min height **256px**, centred; icon tile **36×36**, radius **8**, border 1px `Border`/60, 18px `Muted` icon; title **16px/600 `Text`**; description **12px `Muted`**, max width 384px; action (optional) 28px ghost; gaps 16/4 |
| Loading | keep a 12px `Muted` status line; goble has no shared Skeleton element, and this spec does not add one. T3 uses `Skeleton` at 2.4s; that is a follow-up, not part of D11. |
| Error state | same as empty but icon `alert-triangle` in `Warning`, title names the failure, description carries the retryable reason, action `Retry` 28px |

**Owner files.** `app/src/ui/harness.rs` (`build_page`, `card`, `build_*_row`, the four pages), `crates/goble-ui/src/elements/empty.rs` or a new `empty_state.rs` (the current `Empty` is only a sized spacer, not an empty state), `app/src/integration_testing/harness_pages_flow.rs`.

**Acceptance tests.**

1. `a_harness_page_header_is_48px_with_a_14px_title`: frame Workflows with one seeded workflow; assert the header band height 48, the "Workflows" run `font_size == 14.0`, and the count run `font_size == 11.0`.
2. `harness_rows_are_one_12px_group_not_a_stack_of_cards`: assert exactly one group `StrokeRect { width: 1.0, corner_radius: 12.0 }` for the list, and `n - 1` 1px separators for `n` rows; no per-row `SurfaceRaised` fill with 8px gaps.
3. `the_empty_timeline_is_centred_with_a_16px_title`: open Timeline with no records; assert the empty title `font_size == 16.0`, the description `font_size == 12.0`, the icon tile 36×36, and the block's height ≥ 256.
4. `the_harness_content_is_capped_at_896px`: frame at 1400px; assert the content's left/right insets are equal and the content width is 896.
5. `the_workflows_page_still_renders_real_data`: keep the existing `harness_pages_flow.rs` assertion that a seeded "Nightly digest" is drawn.

Verification: `cargo test -p goble-app --lib ui::harness` (new test module) and `cargo test -p goble-app --test harness_pages_flow`.

### 3.5 Harness / agent surface

This is the surface `build_agent_chat` draws (`app/src/ui/chat/agent.rs`), the one `harness_mode` puts over a shell; the observability pages in §3.4 are its sibling, not its replacement.

**Current, measured:**

- Header: `build_agent_header` pads the bar to `TOPBAR_HEIGHT` (36 macOS / 40 other) around `HEADER_CONTROL_HEIGHT = 32`; the title is a 12px `Text` with `HARNESS_MARK_SIZE = 14`; the expand and close controls are 32px `TopbarButton`s with 16px glyphs; the leading slot is the `esc` hint, the centre is the heading, the trailing is the work chip; spacing is 8px. The bar has no rule of its own; it sits on the pane.
- Empty state (`crates/goble-ui/src/views/chat_view/transcript.rs`, `build_empty_state`): a rounded frame capped at `EMPTY_FRAME_MAX_WIDTH = 720`, 16px padding, a 28px icon, a 12px title and a 12px subtitle; the frame border is `Border` mixed 45% (`EMPTY_FRAME_BORDER_MIX`); it sits a third of the free height above the composer.
- Work dock: `WorkDock` under the header, `Surface` band with `md` horizontal padding, 11–12px rows.

**Target.**

| Part | Value |
|---|---|
| Header height | keep 36 (macOS) / 40 (other); do not grow the pane chrome to T3's 52px workspace bar |
| Header padding | 8px horizontal; 1px bottom `Border`/60 so the header separates from the transcript |
| Title | **14px/600 `Text`** (was 12/400), max width 340 (existing constraint) |
| Harness mark | **16px** (was 14) |
| Expand / close | **32px** hit box, **24×24** drawn control, 16px glyph, `Muted`; hover `Hover`, 2px `Focus` ring when keyboard-focused |
| Work chip | 20px height, 11px `Muted`, no border; opens the work dock |
| Work dock row | 32px min height, 12px/500 title, 11px `Muted` meta, 1px `Border`/50 between rows; `Surface` band, no per-row card |
| Empty state | no outer border/frame; max content width **384px**, centred, min height **240px**; icon tile **36×36**, radius **8**, 1px `Border`/60, 18px `Muted` glyph; title **20px/600 `Text`**; subtitle **14px `Muted`**, max width 384; 12px gap; sits one third of the free height above the composer (keep the existing placement) |
| Empty action | optional 28px ghost button, 12px label; only where the host has one |
| Focus | the pane's existing focus marker/ring is unchanged; header controls get the 2px `Focus` ring from D2 |

**Owner files.** `app/src/ui/chat/header.rs` (`HEADER_CONTROL_HEIGHT`, `HARNESS_MARK_SIZE`, title `Text`, expand/close), `crates/goble-ui/src/views/chat_view/transcript.rs` (`EMPTY_FRAME_MAX_WIDTH`, `EMPTY_FRAME_BORDER_MIX`, `build_empty_state`), `app/src/ui/chat/agent.rs` (passes the empty state).

**Acceptance tests.**

1. `the_agent_header_title_is_14px_with_a_16px_harness_mark`: frame a pane with a heading and a harness; assert the heading `DrawText { font_size: 14.0 }` and the mark's `DrawIcon { size: 16.0 }`.
2. `the_agent_header_has_one_1px_bottom_rule_and_32px_controls`: assert one 1px `StrokeRect` at the bar's bottom; expand/close hit boxes are 32×32 with 16px glyphs.
3. `the_agent_empty_state_is_unframed_and_reads_at_20_14`: assert no `StrokeRect` around the empty content, the title `font_size == 20.0`, subtitle `font_size == 14.0`, icon tile 36×36 with radius 8, and the block's min height ≥ 240.
4. `a_pane_with_no_messages_still_draws_its_composer`: keep the existing empty-state-plus-composer invariant.

Verification: `cargo test -p goble-app --lib ui::chat && cargo test -p goble-ui --lib views::chat_view`.


---

## 4. Discrete items (D1–D15)

Each item is one implementation turn, in dependency order. `layer` is **ui** for pure `goble-ui` (shared) or **app** for the shell. Verification names the test binary/module that must be green; the repo has no GUI runner here, so all of these are drawn-command/state tests, not screenshots.

### D1 — Token layer, contrast math and motion
- **Scope:** add the §2.2 colour/spacing/radius/type/focus/motion tokens, `ColorU::relative_luminance`/`contrast_ratio`/`mix_over`, `Theme::radius_token`/`type_px`/`motion_ms`, light `Focus` `#00c2ff` → `#007a9e`, light `Muted` `#6f6f6f` → `#696969`, `InputBorder`/`Placeholder`/foreground pairs. No component consumes them yet.
- **Files:** `crates/goble-ui/src/theme.rs`, `crates/goble-ui/src/color.rs`, `crates/goble-ui/src/lib.rs` re-exports.
- **Acceptance:** all five presets resolve every `ColorToken`; `contrast_ratio(Text,Bg) >= 4.5`, `contrast_ratio(Muted,SurfaceRaised) >= 4.5`, `contrast_ratio(Focus,Bg) >= 3.0`, `contrast_ratio(InputBorder,Surface) >= 3.0`; `Focus` light is `#007a9e`; `Theme::motion_ms(200)` is 0 when `Motion::reduced`.
- **Verification:** `cargo test -p goble-ui --lib theme && cargo test -p goble-ui --lib color`
- **Depends on:** none. **Layer:** ui.

### D2 — One focus ring and one hit-target rule
- **Scope:** lift the app's `focus_ring` into `goble-ui/src/elements/focus.rs` as `focus_ring(element, focused, style)`; route `Button`, `TextInput`, `SearchInput`, `Checkbox`, `HoverRow`, `Chip`, `ComposerButton`, `TopbarButton` through it; 2px control ring / 3px field ring, inset offset; minimum targets (32px row, 24px icon, 20px close) and an optional `coarse_pointer` 44px hit box.
- **Files:** new `crates/goble-ui/src/elements/focus.rs`, `elements.rs`, the listed element files, `app/src/ui/settings.rs` (replace local helper).
- **Acceptance:** each listed primitive, when focused, paints exactly one `StrokeRect`/ring in `Focus` at the specified width and radius +2; a button with a 10px glyph still lays out at ≥24px; no primitive paints two rings when a field is focused.
- **Verification:** `cargo test -p goble-ui --lib elements`
- **Depends on:** D1. **Layer:** ui.

### D3 — Settings rail
- **Scope:** `NAV_WIDTH` 200, 32px rows, 14px label/selected 500, 2px gap, no dividers, 2px focus ring, 8px rail padding.
- **Files:** `app/src/ui/settings.rs` (`NAV_WIDTH`, `build_nav`), `app/src/ui/settings_category_tests.rs`.
- **Acceptance:** §3.1 test 1; rail fill width 200; focus ring width 2; zero horizontal `Divider`s inside the nav; selected row height 32.
- **Verification:** `cargo test -p goble-app --lib ui::settings_category_tests`
- **Depends on:** D1, D2. **Layer:** app.

### D4 — Settings page, group and row anatomy
- **Scope:** 24px page padding, 896px cap, 16/600 section headers, 12px group card with 1px border and 1px/50 separators, 16/12 row padding, 14/500 label + 12/18 description, 32px label/control gap, 2px row focus. Add per-control description copy and a shared `SettingsRow` (lift `views/settings_view/row.rs` or keep it app-side; one owner).
- **Files:** `app/src/ui/settings.rs` (`build_pane`, `heading`, `setting_row`, `field_row`), `app/src/ui/types.rs` (description mapping), possibly `crates/goble-ui/src/elements/settings_row.rs` + `elements.rs`, `app/src/ui/settings_category_tests.rs`.
- **Acceptance:** §3.1 tests 2–4; a row with a description is 66px, without is 44px; group radius 12; label 14/description 12.
- **Verification:** `cargo test -p goble-app --lib ui::settings_category_tests`
- **Depends on:** D1, D2, D3. **Layer:** app (+ ui if the row is lifted).

### D5 — Settings search, empty state and destructive confirmation
- **Scope:** rail `SearchInput` (32px, `/` hint, clear, results list, no-results), query in `UiState`, Up/Down/Enter/Escape, jump-to-row ring; T3-style empty row (icon + title + description); alert dialog geometry (16px radius, 20/600 title, 14 muted description, 24px padding, destructive action) for reset/delete.
- **Files:** `app/src/ui/settings.rs`, `app/src/state/ui_state/settings.rs`, `app/src/state/types.rs`, `app/src/ui/actions.rs`, `app/src/ui/settings_category_tests.rs`, `crates/goble-ui/src/elements/dialog.rs` (radius/title/description).
- **Acceptance:** typing `cont` filters to Contrast; `/` focuses the field; Escape clears and restores the rail; "No settings found" is drawn at 12px `Muted`; the Connections page with no parsed hosts draws icon/title/description; a delete confirmation draws a 16px-radius dialog with a 20px title and initial focus on Cancel.
- **Verification:** `cargo test -p goble-app --lib ui::settings_category_tests && cargo test -p goble-ui --lib elements::dialog`
- **Depends on:** D3, D4. **Layer:** app (+ ui for dialog).

### D6 — Composer card, padding and focus
- **Scope:** 16px card, 1px `Border`/60, 16/12/8 padding, 2px `Focus` ring on focus, 14px editor text, `Placeholder`, 48–160 height. Update the existing composer tests that count `StrokeRect` as pills.
- **Files:** `crates/goble-ui/src/elements/chat_composer/composer.rs`, `crates/goble-ui/src/elements/chat_composer/tests.rs`, `text_area.rs`.
- **Acceptance:** §3.2 tests 1, 2, 5; existing pill/SSH-chip tests pass with their border counts corrected.
- **Verification:** `cargo test -p goble-ui --lib elements::chat_composer::tests`
- **Depends on:** D1, D2. **Layer:** ui.

### D7 — Composer actions, states and attachments
- **Scope:** 24px/6px/12px context and model pills, 32px send circle (`Accent`/`AccentForeground`), 32px stop (`Error`/`ErrorForeground`), 24px attachment chips with 16px remove, 20px keyboard caps, disabled 64%, narrow wrap; wire the send button to the same `on_send` path as Enter.
- **Files:** `crates/goble-ui/src/elements/chat_composer/composer.rs`, `composer_button.rs`, `context_pill.rs`, `chip.rs`, `app/src/ui/chat/composer.rs`.
- **Acceptance:** §3.2 tests 3, 4, 6; `contrast_ratio(AccentForeground, Accent) >= 4.5` for every `AccentColor` preset; a disabled composer cannot submit.
- **Verification:** `cargo test -p goble-ui --lib elements::chat_composer && cargo test -p goble-ui --lib elements::context_pill && cargo test -p goble-ui --lib elements::chip`
- **Depends on:** D6. **Layer:** ui (+ app one-line wiring).

### D8 — Tab 2px edge and close target
- **Scope:** 2px bottom edge (`Border` rest / `Accent` selected with a 3:1 gate and `Text` fallback), `h_pad` 8, 20px close, content centred in `height - 2`; keep square corners, keep the 1px rule, keep the 60/40/20 tint.
- **Files:** `app/src/ui/shell/topbar.rs`, `app/src/ui/shell/tests.rs`.
- **Acceptance:** §3.3 tests 1–6; existing square-corner test still passes; selected edge contrast ≥ 3.0 or `Text` fallback is used.
- **Verification:** `cargo test -p goble-app --lib ui::shell::tests`
- **Depends on:** D1. **Layer:** app.

### D9 — Tab strip keyboard model
- **Scope:** make the strip a real focus region: `F6`/`Ctrl+Tab` moves focus into it, Left/Right move, Home/End jump, Enter/Space activate, Delete/Backspace close, Escape returns focus to the pane; roving focus (only the active tab in the tab order), 2px `Focus` ring inset 2px. Announce nothing (no screen reader); the visual focus is the contract.
- **Files:** `app/src/ui/shell/topbar.rs`, `app/src/state/types.rs` (`tab_strip_focus`), `app/src/root_view/element.rs`, `app/src/ui/shell/tests.rs`, `app/src/root_view/tests/topbar.rs`.
- **Acceptance:** after `F6`, the active tab paints a 2px `Focus` ring; Right selects/draws the next tab; Home/End reach the first/last; Delete closes and focus lands on the neighbour; Escape returns focus to the active pane without closing the tab; `Ctrl+Tab` still switches tabs.
- **Verification:** `cargo test -p goble-app --lib topbar`
- **Depends on:** D8. **Layer:** app.

### D10 — Harness header and page container
- **Scope:** 48px header with 14/600 title and 11px count, 28px ghost Back, 16px horizontal padding, 1px bottom rule; 896px content cap, 20/24/48 padding, 24px section gap.
- **Files:** `app/src/ui/harness.rs` (`build_page`), new `#[cfg(test)] mod tests` in that file, `app/src/integration_testing/harness_pages_flow.rs`.
- **Acceptance:** §3.4 tests 1 and 4; the existing seeded-workflow integration test still draws the real row.
- **Verification:** `cargo test -p goble-app --lib ui::harness` and `cargo test -p goble-app --test harness_pages_flow`
- **Depends on:** D1, D2. **Layer:** app.

### D11 — Harness group cards, rows and empty state
- **Scope:** one 12px group with 1px border and 1px/50 separators instead of a stack of raised cards; 48/64px rows with 14/500 title + 12 metadata; 16px horizontal / 12px vertical; a real empty state (36px icon tile, 16/600 title, 12 description, 256px min height, optional action). No skeleton element in this item.
- **Files:** `app/src/ui/harness.rs` (`card`, `build_*_row`, each page), `crates/goble-ui/src/elements/empty_state.rs` (new) + `elements.rs`, `crates/goble-ui/src/elements/empty.rs` (keep as spacer).
- **Acceptance:** §3.4 tests 2, 3, 5; a list of 3 rows draws exactly one group border and 2 separators.
- **Verification:** `cargo test -p goble-app --lib ui::harness` and `cargo test -p goble-ui --lib elements::empty_state`
- **Depends on:** D10. **Layer:** app (+ new ui element).

### D12 — Appearance: text scale and contrast mode
- **Scope:** a `Text size` control **12–20px** (default 16) applied through `Theme::text_scale`; a `Contrast` control **Normal / High** (or 100/150/200) applied through `Contrast`; persist in config; expose both controls in the Appearance page with reset-to-default. No page zoom.
- **Files:** `app/src/state/types.rs`, `app/src/state/ui_state/settings.rs` (or config), `app/src/ui/settings.rs` (Appearance page), `app/src/actions/make_actions.rs`, `crates/goble-ui/src/theme.rs` (already has the fields from D1).
- **Acceptance:** setting text scale 20 makes `Theme::type_px(Label) == 17.5` (14 × 1.25) or an equivalent clamped scale; `Contrast::High` substitutes `BorderStrong` and raises focus width to 3; values survive a tree rebuild and a config reload; a reset returns 16/Normal.
- **Verification:** `cargo test -p goble-app --lib ui::settings_category_tests` and `cargo test -p goble-ui --lib theme::tests`
- **Depends on:** D1, D4. **Layer:** app.

### D13 — Reduced motion from the platform
- **Scope:** query the OS preference once at startup (macOS `NSWorkspace.accessibilityDisplayShouldReduceMotion` via the existing `objc2` path; Windows `UISettings::AnimationsEnabled`; Linux best-effort `org.freedesktop.portal.Settings` or default false), feed `Theme::motion.reduced`; add an override switch in Appearance (System / On / Off); when reduced, all durations are 0 and scroll easing is instant.
- **Files:** new `app/src/platform/a11y.rs` (or `app/src/platform/mod.rs`), `app/src/main.rs`, `app/src/ui/settings.rs`, `crates/goble-ui/src/theme.rs`, `crates/goble-ui/src/elements/scrollable.rs`.
- **Acceptance:** reduced mode makes `motion_ms(200) == 0` and makes `ScrollState::scroll_by_smooth` behave like `scroll_by`; the override persists and wins over the platform value.
- **Verification:** `cargo test -p goble-ui --lib theme && cargo test -p goble-ui --lib scrollable && cargo test -p goble-app --lib platform::a11y`
- **Depends on:** D1, D12. **Layer:** app + ui.

### D14 — Agent/harness surface header and empty state
- **Scope:** agent bar title 14/600, 16px harness mark, 32px controls with 24px drawn close, 1px bottom rule; work dock rows 32px/12/11 with 1px rules; chat empty state unframed, 384px content, 240px min height, 36px icon tile, 20/600 title and 14px subtitle. Keep the 36/40 pane bar height.
- **Files:** `app/src/ui/chat/header.rs`, `crates/goble-ui/src/views/chat_view/transcript.rs`, `app/src/ui/chat/agent.rs`, tests in both crates.
- **Acceptance:** §3.5 tests 1–4; the pane header still fits 36px on macOS.
- **Verification:** `cargo test -p goble-app --lib ui::chat && cargo test -p goble-ui --lib views::chat_view`
- **Depends on:** D1, D2, D10, D11. **Layer:** app (+ ui).

### D15 — Accessibility regression suite

- **Scope:** test-only. A table of each text token against the surfaces it is actually drawn on (all themes): `Text` on `Bg`/`Surface`/`SurfaceRaised`/`Hover`/`Selected`; `Muted` and `Placeholder` on `Bg`/`Surface`/`SurfaceRaised`/`Hover` (not on `Selected`, which pairs with `Text`), asserting **≥ 4.5:1**; every control boundary/focus pair (`InputBorder`, `Focus`, tab edge) asserting **≥ 3.0:1**; a focus-ring geometry test per primitive; a keyboard-reachability test that every `SettingsControl` in `pane_controls` is drawn and every tab is reachable; an explicit "no screen reader" note asserted only as a doc check (compile-time constant in the test file), not as a feature test.
- **Files:** `crates/goble-ui/src/theme.rs` (`#[cfg(test)] mod contrast_tests`), `app/src/ui/a11y_tests.rs` + `app/src/ui/mod.rs`, `app/src/integration_testing/harness_pages_flow.rs`.
- **Acceptance:** the matrix is non-vacuous (a deliberately dimmed `Muted` fails), all current themes pass, and the test names are the ones quoted in §3.
- **Verification:** `cargo test -p goble-ui --lib theme && cargo test -p goble-app --lib a11y_tests`
- **Depends on:** D1–D14. **Layer:** tests only.

---

## 5. Open questions for the owner

1. **The 2px tab edge is a trial.** Is it a permanent replacement for the fill-only active tab, or an experiment to review on the running app? The spec assumes it **supplements** the `SurfaceRaised` selected fill and the existing colour tint; it does not replace either.
2. **Selected edge colour when a tab is tinted.** The spec uses `Accent` always and keeps the tab colour in the fill only. Should a red-tinted selected tab get a red edge, or is the edge a global selection marker? Also: in the **light** theme `Accent #9a9a9a` is 2.47:1 on `SurfaceRaised`, below 3:1; should D8 darken the light accent, use `Text` as the edge, or accept the trial as visual-only?
3. **Rail width:** 200px is a compromise between goble's 148 and T3's 240. Do you want T3's 240 with 16px settings icons (new icon assets), or keep a label-only rail at 200?
4. **Grouped settings cards:** T3 groups rows into rounded 12px cards with 1px borders. Goble's current settings is a flat list with a divider per row. The spec assumes grouped cards; confirm that this is the "friendlier" reading, or keep the flat list and borrow only the type/spacing.
5. **Composer send button:** goble currently has no visible send control (Enter submits). D7 adds a 32px button. Is a visible send/stop affordance wanted, and should it duplicate Enter or replace it?
6. **Type scale divergence:** T3's 6/10/14px half-steps are rounded to 8/12/16 on goble's 4px grid. Is that acceptable, or do you want a 2px-granular spacing scale (which would add `6`, `10`, `14` tokens)?
7. **Accent palette:** the default accent `#9a9a9a` is deliberately neutral. The recommended light `Focus #007a9e` and `InputBorder #8a8a8a` are more saturated/darker than T3's zinc values. Are those acceptable, or should goble keep its current values and rely on the user contrast setting?
8. **High contrast model:** T3 exposes 50–200%; goble can expose Normal/High only, or a 100/150/200 slider. Which is the product's expectation for a custom renderer?
9. **Reduced motion on Linux/Windows:** macOS has a clean `NSWorkspace` query; Windows/Linux need platform-specific probes. Is a macOS-first implementation acceptable, with a manual switch everywhere else?
10. **Screen-reader statement:** the spec proposes an explicit release-note sentence that goble is not screen-reader-accessible. Is the owner willing to publish that, or should the `accesskit` bridge be scheduled first?
11. **Shadows/glass:** T3's composer/dialog use soft shadows and backdrop blur. The spec drops both (no wgpu blur at this layer) and uses borders. Is a flat, bordered look acceptable, or should a GPU blur pass be scoped?
