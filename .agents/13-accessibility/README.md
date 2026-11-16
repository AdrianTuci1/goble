# 13 — Accessibility (TS clients)

**Status:** `[~]` first pass landed — tokens, focus, names and a contrast gate; no audit against a real screen reader yet
**Owns:** the accessibility rules the browser and phone shells are held to
**Applies to:** `apps/web` and `apps/mobile` (the TypeScript clients)

The Rust desktop app (`app/`, `crates/goble-ui`) is **out of scope** here: it
draws its own frames and has no accessibility layer (no AT-SPI, no VoiceOver),
which is a separate project, not a token change.

## Contrast

`apps/common/src/lib/theme.ts` is the palette both TS shells consume by name.
`npm run a11y:contrast` (`scripts/check-contrast.mjs`) reads it and fails CI on
any of:

- `text`, `muted`, `subtle`, `accent`, `success`, `warning`, `error` below
  **4.5:1** on each of `bg`, `surface`, `raised`, `overlay`;
- the tinted pairs `error`/`errorSoft`, `success`/`successSoft`,
  `warning`/`warningSoft`, `accent`/`accentSoft`, `text`/`accentSoft` below
  **4.5:1**;
- the destructive label pair `bg` on `error` below **4.5:1**;
- `borderInteractive` below **3:1** on `bg`, `surface`, `raised`.

A missing expected token fails the script instead of being skipped. Current
dark values and every printed ratio are in
[`../06-renderer/ui-spec/01-design-tokens.md`](../06-renderer/ui-spec/01-design-tokens.md).

Two rules follow from the numbers:

- `subtle` (`#8b8b8b`) is the *dimmest* text colour allowed, and only at
  10–12 px where it still clears 4.5:1 (4.56:1 on `overlay`). Anything dimmer
  must not carry text.
- `border` (`#262626`) is decorative. A border that is a control's only
  affordance uses `borderInteractive` (`#6a6a6a`, 3.08:1 on `raised`).

`error` moved from `#ef4444` to `#f87171` because the old red was 4.43:1 on
`raised` and 4.13:1 on `overlay`. Its white-on-red fill is gone: the destructive
button's label is `bg` on `error` (6.98:1).

## Focus

The browser gets **one** stylesheet, injected by `apps/web/lib/focus.ts`
(`useFocusVisible`, mounted in `apps/web/app/_layout.tsx`):

```
:focus-visible { outline: 2px solid palette.accent; outline-offset: 2px; }
@media (prefers-reduced-motion: reduce) { …animations/transitions/smooth scroll off… }
```

No component repeats a focus rule, and no style may set `outline: none` /
`outlineWidth: 0` on an interactive element — the text inputs in `composer`,
`conversation-pane`, `conversation-menu`, `server-sheet` and `terminal-surface`
had that reset and it is removed. Keyboard activation is react-native-web's own:
`Pressable` with `accessibilityRole="button"` renders a `<button tabindex="0">`
and answers Enter and Space.

## Names

Every interactive element must be announced with a name. The rules:

- an icon-only `Pressable` carries `accessibilityLabel`, and its `Ionicons`
  child is `aria-hidden` (web) / `accessible={false}` (native) so the glyph is
  not read twice;
- state that a tint conveys (enabled/paused, running/failed, selected) is also in
  `accessibilityState` or the label, never colour alone;
- decorative glyphs are hidden, not labelled;
- a control under 44×44 gets `hitSlop` rather than a bigger visual box.

## What is not done

- no screen-reader pass (NVDA/VoiceOver/TalkBack) and no browser or device was
  available in the pass that wrote this;
- no automated a11y linter (`eslint-plugin-jsx-a11y`) or axe run over the export;
- tab order on the web shell is untested end to end;
- dynamic type / font scaling on the phone is not audited;
- the Rust desktop app has no accessibility layer at all.
