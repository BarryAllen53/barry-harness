# Barry design system

Status: design specification. Implementation tokens: `frontend/styles/tokens.css`. Icons: `frontend/assets/icons/`, documented in `icons.md`. Behaviour and layouts: `ux.md`.

Targets: WCAG 2.2 AA everywhere, AAA contrast (7:1) for body text and primary UI text in the light, dark and high-contrast themes where feasible, and exceptions are stated explicitly below. All ratios were computed with the WCAG relative-luminance formula from the hex values in `tokens.css`.

## 1. Visual identity: "Barry"

Barry is a calm, capable workshop partner. The product handles long, serious work (code, files, commands), so the interface is quiet, dense where it needs to be and never decorative at the cost of legibility.

- Character: warm paper and ink, with one honey-amber accent. Not the cold blue-grey of most developer tools, not neon.
- Name and mark: a bear head drawn with the same 1.75 stroke as the icons (`logo-mark.svg`). Round ears, two eyes, a small nose. Friendly, drawn in one weight, reads at 16 px.
- Accent: honey amber. In light it is a burnt amber (`#8F4300`) so that white text passes AAA on it. In dark it is bright honey (`#F2A444`) with near-black text. Amber is used for the primary action, the current location and the focus of the product. It never signals state.
- State colours (success green, warning ochre, danger red, info blue) are used only together with an icon and a text label.
- Typography: Segoe UI Variable, because the app runs on Windows 11 WebView2 and it is already installed, optically sized and crisp at small sizes. Display text uses the Display optical size at semibold with slight negative tracking. Code uses Cascadia Mono.
- Shape: soft but not bubbly. 10 px radius on surfaces, 6 px on controls, pills only for chips.
- Surfaces: flat. Separation comes from tone and 1 px borders, with shadow only on floating layers (menus, palette, dialogs). Dark theme replaces shadow with a lighter surface and a visible border.
- Layout: a strong reading column. Transcript text is held to 72 characters; code, diffs and media may be wider.
- Voice: short, concrete, first person for the agent ("I changed 2 files"), second person for instructions. No exclamation marks, no mascot chatter in alerts or errors. The bear appears in the header, the empty states and the about screen only.
- Motion: purposeful and brief. Streaming text appears word by word without animation of layout. Panels slide 8 px and fade in 140 to 220 ms. Everything stops under `prefers-reduced-motion`.
- Do: use whitespace and alignment to group, keep one primary action per view, show state as text first and decoration second.
- Do not: use gradients behind text, use colour alone for meaning, use motion to hold attention, auto-play media, or show a spinner without text.

## 2. Colour tokens

Themes: `light` (default when the system is light), `dark` (default when the system is dark), `contrast` (user-selected high-contrast theme), and `forced-colors` (Windows contrast themes, mapped to system colours). Precedence: an explicit `data-theme` on `<html>` wins over the system preference. `forced-colors: active` always overrides everything. `prefers-contrast: more` strengthens borders and subtle text when no explicit theme is set.

### 2.1 Surfaces and text

| Token | Light | Dark | Contrast |
|---|---|---|---|
| `--color-bg` | `#FBFAF6` | `#12110D` | `#000000` |
| `--color-surface` | `#FFFFFF` | `#1B1A15` | `#0A0A0A` |
| `--color-sunken` | `#F1EFE8` | `#0C0B08` | `#000000` |
| `--color-text` | `#15140F` | `#F2F0E7` | `#FFFFFF` |
| `--color-text-muted` | `#46443B` | `#C9C6B9` | `#F0F0F0` |
| `--color-text-subtle` | `#5C5A50` | `#A8A598` | `#DCDCDC` |
| `--color-border` | `#75726A` | `#8A877B` | `#FFFFFF` |
| `--color-border-subtle` | `#D9D6CB` | `#34322A` | `#9A9A9A` |
| `--color-code-bg` | `#F1EFE8` | `#0C0B08` | `#000000` |
| `--color-focus` | `#1D4ED8` | `#8FB4FF` | `#5CE1FF` |

### 2.2 Accent and state

| Token | Light | Dark | Contrast |
|---|---|---|---|
| `--color-accent` | `#8F4300` | `#F2A444` | `#FFD60A` |
| `--color-accent-hover` | `#7A3900` | `#F7B766` | `#FFE766` |
| `--color-accent-active` | `#663000` | `#FFC986` | `#FFF0A0` |
| `--color-accent-fg` | `#FFFFFF` | `#1A0F00` | `#000000` |
| `--color-accent-soft` / `-soft-text` | `#FBEBD3` / `#6B3200` | `#3A2508` / `#FFD9A3` | `#1A1400` / `#FFE766` |
| `--color-success` / `-soft` | `#1B6B3A` / `#E3F3E6` | `#6FD08C` / `#12301C` | `#5CFF8A` / `#00200C` |
| `--color-warning` / `-soft` | `#7A4E00` / `#FFF1CC` | `#F0B94A` / `#35280A` | `#FFC83D` / `#2A1F00` |
| `--color-danger` / `-soft` | `#B3261E` / `#FBE4E1` | `#FF8A80` / `#3A1512` | `#FF8A8A` / `#2A0000` |
| `--color-info` / `-soft` | `#1F5FA8` / `#E2EDFB` | `#8FBFFF` / `#10243E` | `#7DD3FF` / `#001A2A` |
| `--color-diff-add-bg` / `-text` | `#E3F3E6` / `#0F4D26` | `#12301C` / `#A8EBBD` | `#00200C` / `#9DFFBA` |
| `--color-diff-remove-bg` / `-text` | `#FBE4E1` / `#8A1C14` | `#3A1512` / `#FFC2BC` | `#2A0000` / `#FFB3B3` |

Interaction overlays are theme-agnostic: `--color-hover` is 7 percent of text colour, `--color-pressed` is 13 percent. Disabled controls use `--color-disabled-text` on `--color-disabled-bg` and additionally drop the border to dashed, so disabled never relies on faintness alone.

### 2.3 Contrast ratios (computed)

Text pairs (AA needs 4.5, AAA needs 7):

| Pair | Light | Dark | Contrast | Level |
|---|---|---|---|---|
| text on bg | 17.66 | 16.55 | 21.00 | AAA |
| text on surface | 18.44 | 15.26 | 19.80 | AAA |
| text on sunken / code | 16.03 | 17.24 | 21.00 | AAA |
| text-muted on bg | 9.35 | 11.03 | 18.43 | AAA |
| text-muted on surface | 9.77 | 10.17 | 17.37 | AAA |
| text-muted on sunken | 8.49 | 11.49 | 15.31 | AAA |
| text-subtle on bg | 6.63 | 7.65 | 15.31 | AA light (6.63), AAA dark and contrast |
| text-subtle on surface | 6.92 | 7.05 | 14.44 | AA light (6.92), AAA dark and contrast |
| text-subtle on sunken | 6.02 | 7.97 | 15.31 | AA light, AAA dark and contrast |
| text-subtle on accent-soft | 5.91 | 5.87 | 13.39 | AA light and dark, AAA contrast |
| accent on bg (links, text buttons) | 6.77 | 9.15 | 14.88 | AA light (6.77), AAA dark and contrast |
| accent on surface | 7.07 | 8.44 | 14.02 | AAA |
| accent-fg on accent (primary button) | 7.07 | 9.14 | 14.88 | AAA |
| accent-fg on accent-hover | 8.70 | 10.72 | 16.87 | AAA |
| accent-fg on accent-active | 10.56 | 12.54 | 18.22 | AAA |
| accent-soft-text on accent-soft | 8.61 | 10.85 | 14.75 | AAA |
| success on surface | 6.54 | 9.19 | 15.20 | AA light, AAA dark and contrast |
| warning on surface | 7.20 | 9.74 | 12.80 | AAA |
| danger on surface | 6.54 | 7.63 | 8.72 | AA light, AAA dark and contrast |
| info on surface | 6.44 | 9.18 | 11.91 | AA light, AAA dark and contrast |
| success on success-soft | 5.68 | 7.56 | 13.31 | AA light, AAA dark and contrast |
| warning on warning-soft | 6.41 | 8.05 | 10.49 | AA light, AAA dark and contrast |
| danger on danger-soft | 5.38 | 7.10 | 8.42 | AA light, AAA dark and contrast |
| info on info-soft | 5.44 | 8.23 | 10.70 | AA light, AAA dark and contrast |
| diff-add-text on diff-add-bg | 8.64 | 10.43 | 14.40 | AAA |
| diff-remove-text on diff-remove-bg | 7.66 | 10.57 | 11.24 | AAA |
| selection-text on selection-bg | 15.74 (`#15140F` on `#FBEBD3`) | 8.98 | black on `#FFD60A` = 14.88 | AAA |
| disabled-text on disabled-bg | 4.87 | 4.55 | 8.03 | AA, and exempt from WCAG regardless |

Where the table says AA for light theme, the pair is used for secondary, non-body text (timestamps, captions, state chips with an icon and a label, tool metadata). Body text, the composer, message text, dialogs and form labels use `--color-text` or `--color-text-muted`, both AAA in every theme.

Non-text pairs (3:1 needed, WCAG 1.4.11):

| Pair | Light | Dark | Contrast |
|---|---|---|---|
| border on bg (input and control borders) | 4.60 | 5.25 | 21.00 |
| border on surface | 4.80 | 4.84 | 19.80 |
| border on sunken | 4.18 | 5.47 | 21.00 |
| focus ring on bg | 6.42 | 9.12 | 13.67 |
| focus ring on surface | 6.70 | 8.41 | 12.89 |
| focus ring on sunken | 5.82 | 9.50 | 13.67 |
| accent as a 3:1 graphic on bg | 6.77 | 9.15 | 14.88 |
| border-subtle on bg | 1.39 | 1.47 | 7.46 |

`--color-border-subtle` is decorative only (separators between sections that are already separated by whitespace and headings). It must never be the only boundary of an input, button, card that is interactive or any control, and it is not used for that anywhere.

Focus ring over an accent-filled button: the ring is drawn with a 2 px offset, so it sits on the surrounding background, not on the accent fill. Ring against accent measures 1.05 (light) and 1.00 (dark), which is why the offset is mandatory and is part of the token (`--focus-offset: 2px`). The ring is also 3 px wide (`--focus-width`), above the 2 px minimum area requirement of WCAG 2.4.13.

### 2.4 Forced colours

Under `forced-colors: active` the tokens remap to system colours: `Canvas`, `CanvasText`, `ButtonFace`, `ButtonText`, `Highlight`, `HighlightText`, `GrayText`. Rules:

- Do not set `forced-color-adjust: none` except on the contrast theme swatches in Settings and on media thumbnails.
- Borders are required on buttons, inputs, cards and chips (they carry the shape in forced colours, since backgrounds are removed). Use `border: var(--border-width) solid var(--color-border)` on every control; in normal themes it still looks right.
- Selected state must not rely on background: current navigation item gets a 3 px inline-start border (`border-inline-start`), the active tab gets a bottom border, a checked box gets a glyph. All survive forced colours.
- Diff rows use the textual "Added:" and "Removed:" labels plus the visible `+` and `-` gutter glyphs. Backgrounds are decorative.
- Status chips contain an icon (currentColor) and text. No colour dependence.
- Focus: `outline` (never `box-shadow` alone), colour `Highlight`.
- Icons use `currentColor`, so they adopt `ButtonText` or `CanvasText` automatically.
- Scrims become `Canvas` with the dialog bordered, so a modal is still distinguishable from the page.

### 2.5 Colour usage rules

- Meaning is never carried by colour alone. Every state has an icon and a word.
- Text on tinted backgrounds uses the matching `*-text` or `*-soft` pair from the table, not an arbitrary mix.
- Links inside prose are underlined at rest. Underline is removed only in navigation components.
- Terminal and code colouring: ANSI colours map to semantic tokens (red to danger, green to success, yellow to warning, blue to info, others to text). ANSI is never the sole carrier of meaning, and the terminal transcript is plain text.

## 3. Typography

Font stacks (from `tokens.css`):

- Sans: `"Segoe UI Variable Text", "Segoe UI", system-ui, -apple-system, "Helvetica Neue", Arial, sans-serif`
- Display: `"Segoe UI Variable Display", "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif`
- Mono: `"Cascadia Mono", "Cascadia Code", Consolas, "SF Mono", Menlo, "Liberation Mono", monospace`

No web fonts are loaded: zero layout shift, no network dependency, and the user's system fonts respect their Windows text-size and ClearType settings.

Scale (fluid with `clamp`, rem-based so it follows user font-size settings and browser zoom):

| Token | Range (px at 16 px root) | Use |
|---|---|---|
| `--text-xs` | 12 to 13 | Timestamps, kbd chips, badges. Never for essential instructions. |
| `--text-sm` | 13 to 14 | Metadata, sidebar secondary lines, table cells in dense diffs |
| `--text-base` | 16 to 17 | Body, transcript, composer, form fields |
| `--text-md` | 18 to 20 | Card titles, dialog titles, section headings in settings |
| `--text-lg` | 21 to 24 | Page headings (chat title) |
| `--text-xl` | 26 to 32 | Empty state headings |
| `--text-2xl` | 32 to 44 | Welcome screen only |
| `--text-code` | 14 to 15 | Code, diffs, terminal |

Line height: `--leading-normal` 1.55 for prose, `--leading-snug` 1.35 for titles and controls, `--leading-tight` 1.2 for display, `--leading-code` 1.5 for code. Measure: `--measure` 72ch for transcript prose. Weights: 400 body, 500 labels, 600 headings and buttons, 700 only in the wordmark.

Requirements:

- All layouts must survive WCAG 1.4.12 text spacing overrides (line height 1.5, paragraph 2 em, letter 0.12 em, word 0.16 em) without clipping: no fixed heights on text containers, `min-block-size` instead of `block-size`.
- Interface text minimum is 12 px (`--text-xs`), and only for non-essential metadata.
- User setting "Text size" scales the root font-size from 87.5 percent to 150 percent. Everything uses rem, and layouts must hold at 150 percent; this is checked manually, there is no automated layout test.
- Numbers in tables and cost figures use `font-variant-numeric: tabular-nums`.

## 4. Spacing, sizing and shape

Spacing (4 px grid with a 2 px half-step):

| Token | rem | px |
|---|---|---|
| `--space-1` | 0.125 | 2 |
| `--space-2` | 0.25 | 4 |
| `--space-3` | 0.5 | 8 |
| `--space-4` | 0.75 | 12 |
| `--space-5` | 1 | 16 |
| `--space-6` | 1.5 | 24 |
| `--space-7` | 2 | 32 |
| `--space-8` | 3 | 48 |
| `--space-9` | 4 | 64 |
| `--space-10` | 6 | 96 |

Rhythm: inside controls use space-3 and space-4, between related items space-3, between groups space-5 and space-6, between regions space-7 and up. Gutters on small screens are space-5 (16 px).

Target size: `--target-min` 24 px is the floor (WCAG 2.5.8, dense toolbars, diff row actions and chips). `--target-comfortable` 44 px is the default for buttons, tree rows, palette options, tabs and anything a touch screen can reach (Windows 11 tablets and touch laptops). Targets smaller than 44 px have at least 8 px of dead space to their neighbours or are inline in text.

Icon sizes: 16 (chips), 20 (default), 24 (toolbar), 32 (empty states). Stroke 1.75 in the SVG viewBox units, kept with `vector-effect` unmodified, so rendered stroke scales with size and stays proportionate.

Radii: `--radius-xs` 4 (kbd, inline code), `--radius-sm` 6 (buttons, inputs), `--radius-md` 10 (cards, tool-call cards, code blocks), `--radius-lg` 14 (dialogs, palette), `--radius-xl` 20 (media lightbox), `--radius-pill` (chips).

Borders: `--border-width` 1 px (2 px in contrast theme and with `prefers-contrast: more`). `--border-width-strong` 2 px for selected state outlines.

Layout tokens: rail 56 px, sidebar `clamp(16rem, 22vw, 20rem)`, inspector `clamp(20rem, 30vw, 28rem)`, header 52 px, composer maximum width 56 rem, dialog width `min(36rem, 100vw - 2rem)`, palette width `min(40rem, 100vw - 2rem)`.

## 5. Elevation

| Token | Use | Dark and contrast behaviour |
|---|---|---|
| none | page, sidebar, transcript | |
| `--shadow-1` | cards, composer | dark: darker shadow plus visible border. contrast: none, 2 px border |
| `--shadow-2` | menus, popovers, tooltips | same |
| `--shadow-3` | inspector overlay on narrow screens, toasts | same |
| `--shadow-dialog` | native dialogs, palette | same, with `::backdrop` using `--color-scrim` |

Elevation is a visual hint only. Layering never depends on shadow being visible, because every floating layer has a 1 px border in `--color-border`.

## 6. Motion

| Token | Value | Use |
|---|---|---|
| `--duration-instant` | 80 ms | Hover, pressed, checkbox |
| `--duration-fast` | 140 ms | Disclosure, tooltip, chip appearance |
| `--duration-base` | 220 ms | Panel slide, dialog open, palette |
| `--duration-slow` | 360 ms | Reserved: skeleton shimmer cycle, long layout shifts |
| `--ease-standard` | cubic-bezier(0.2, 0, 0, 1) | Movement within the screen |
| `--ease-enter` | cubic-bezier(0, 0, 0.2, 1) | Elements arriving |
| `--ease-exit` | cubic-bezier(0.4, 0, 1, 1) | Elements leaving |

Rules:

- Animate only `transform` and `opacity`. Never animate layout properties in the transcript.
- Every animation is interruptible and finishes at its end state if cancelled. A dialog opens in 220 ms or less and is operable from the first frame (focus moves on `showModal()`, not at animation end).
- `prefers-reduced-motion: reduce` sets all four durations to 0.01 ms in `tokens.css`. In addition, the spinner becomes static, the streaming caret does not blink, skeleton shimmer is replaced by a static tint, and smooth scroll is disabled.
- No parallax, no autoplaying loops. Nothing flashes more than three times per second.
- Auto-scrolling of the transcript during streaming follows a "stick to bottom" rule only while the user has not scrolled up. Any manual scroll, keyboard navigation inside the transcript or focus inside it pauses following, and a "Jump to latest" button appears. In reduced motion the jump is instant.

## 7. Component inventory

Notation: states are Default, Hover, Focus (visible ring), Active (pressed), Disabled, and the extras listed. All components must pass in light, dark, contrast and forced colours, at 200 percent and 400 percent zoom, and be operable by keyboard alone.

### 7.1 Controls

| Component | States | ARIA pattern and notes |
|---|---|---|
| Button, primary / secondary / ghost / danger | Default, hover, focus, active, disabled, busy | Native `<button type="button">`. Min size 44 by 44 for primary, 24 by 24 absolute floor. Disabled uses `aria-disabled="true"` plus `aria-describedby` reason for buttons that explain themselves (Send); other disabled buttons use the native `disabled` attribute. Busy: `aria-busy="true"`, the label stays, a visible "Working" appears in the text. |
| Icon button | Same | Native button, icon `aria-hidden`, accessible name from visually hidden text. Tooltip duplicate of the name. See `icons.md`. |
| Toggle button | Pressed, not pressed | Native button with `aria-pressed`. Constant name. Example: "Show reasoning". |
| Switch | On, off, disabled | Native `<input type="checkbox" role="switch">` with a visible `<label>`. State is also written next to it as text ("On", "Off") so the state does not rely on the thumb position. |
| Checkbox | Checked, unchecked, mixed, disabled, invalid | Native `<input type="checkbox">`, label associated. Mixed with `indeterminate` for group selection. |
| Radio group | Selected, disabled | Native radios in `<fieldset><legend>`. Theme and send-key choices use it. |
| Select | Default, open, invalid | Native `<select>` first. A custom listbox only for the model picker (needs descriptions), using the select-only combobox pattern. |
| Text input | Default, focus, filled, invalid, read-only, disabled | `<label for>` always visible. Hint text and error text referenced by `aria-describedby`. Invalid sets `aria-invalid="true"` and an error message with an icon and the word "Error:" prefixed. |
| Secret input (API key) | Masked, revealed | `type="password"` with a "Show key" toggle button using `aria-pressed`. Value never copied into the accessible tree except while revealed. The stored key is displayed masked with the last four characters. |
| Composer textarea | Empty, typing, generating, disabled (offline), over limit | Native `<textarea>`, label "Message to Barry" (visually hidden label, visible placeholder is a hint only and not the name). Auto-grows to 12 lines then scrolls. Keeps focus when Send, Stop or generation state changes. Keydown handling honours IME composition. |
| Search field | Empty, filled, loading | `<input type="search">` with label, results count announced politely via the local status region. |
| Slider and stepper (text size) | Default, focus, disabled | Native `<input type="range">` with output value text and a number input alternative. |

### 7.2 Navigation and containers

| Component | States | ARIA pattern and notes |
|---|---|---|
| Skip link | Hidden, focused | First element in DOM, shown on focus. "Skip to conversation". Second link: "Skip to message box". |
| Project tree (sidebar) | Item default, hover, focus, selected, expanded, collapsed, loading children, renaming | `role="tree"` with `treeitem`, `group`, roving tabindex, `aria-expanded`, `aria-selected` for the selection (single-select), `aria-current="page"` for the open chat. Arrow keys, Home, End, Enter, F2 rename. Because this is the least robust pattern across screen readers, a flat fallback is specified: a `<ul>` of `<button>` rows inside `<nav>`. The matrix decides which ships (see `ux.md`, test case SR-03). |
| Tabs (inspector) | Default, hover, focus, selected, disabled | Automatic activation tabs pattern: `tablist`, `tab`, `tabpanel`, `aria-selected`, `aria-controls`, roving tabindex. Left and Right move. Panel has `tabindex="0"` when it contains no focusable element. |
| Disclosure | Collapsed, expanded | Native `<details>/<summary>` for reasoning and tool-call details (works in all three screen readers). Where animation is needed, a button with `aria-expanded` and `aria-controls`. |
| Message | User, assistant, system, streaming, complete, error, stopped | `<article>` with `aria-labelledby` pointing at a visually hidden `<h3>` ("You, 14:32" or "Barry, 14:33"). Visible sender label as well. Streaming state: `aria-busy="true"` on the article, no live region. |
| Tool-call card | Pending, awaiting permission, running, succeeded, failed, denied, cancelled | `<li>` in a labelled list ("Tool calls") containing a heading level 4 (tool name), a status chip, and a `<details>` for arguments and result. Summary: "Edit file src/main.rs, succeeded, 0.4 seconds". |
| Status chip | Neutral, success, warning, danger, info, running | Icon `aria-hidden`, visible text, never colour alone. Text is always part of the DOM. |
| Reasoning panel | Collapsed, expanded, streaming, complete | `<details>` titled "Reasoning" with elapsed time. Streaming visual text is `aria-hidden="true"` until the reasoning is complete, then replaced once by a static copy. Default collapsed after completion. |
| Diff viewer | Unified, split (visual only), summary list; file collapsed or expanded; hunk focused | `<table>` with `<caption>`, `<th scope="col">`, hunk header rows as `<th scope="colgroup">`, hidden text "Added:" and "Removed:" in the first cell of changed rows. Summary list view as alternative. Horizontal overflow only inside the table wrapper, which is a focusable named region. |
| Terminal pane | Idle, running, finished (success, failure), truncated | Region labelled "Terminal output". Each command is an `<article>` with heading (command), status, `<pre tabindex="0">` output. Running output is visual only and `aria-hidden`, text becomes available at completion. |
| Code block | Default, focused, copied, wrapped | `<figure>` with `<figcaption>` (language and file name), `<pre tabindex="0">` named region, copy button. Wrap toggle. |
| Banner | Info, success, warning, error | `<section>` or `<div>` with heading-less text and an icon. Not a live region by itself. Announced via the announcer. Dismissible banners have a labelled close button. |
| Toast | Info, success | Visual only, `aria-hidden="true"`, auto-dismiss 6 s with pause on hover and focus, no actions. Text goes through the announcer. Errors are never toasts: they stay in the transcript or a banner. |
| Tooltip | Hidden, visible | Shown on hover and keyboard focus, dismissible with Escape, hoverable (WCAG 1.4.13), `role="tooltip"` linked through `aria-describedby`. Never the only place a name or instruction exists. |
| Skeleton | Loading | Visual shapes `aria-hidden="true"`. The container carries `aria-busy="true"` and the announcer says "Loading projects". Shimmer off under reduced motion. |
| Progress | Determinate, indeterminate | `role="progressbar"` with value, or text "Working" for indeterminate. Percent changes are not announced continuously: announced at 25 percent steps at most through the announcer. |
| Badge and kbd chip | Default | `<kbd>` for keys, visible in tooltips and the shortcut dialog. |

### 7.3 Overlays

| Component | States | ARIA pattern and notes |
|---|---|---|
| Dialog (confirm, permission, settings-section edit) | Opening, open, closing | Native `<dialog>` opened with `showModal()`. `aria-labelledby` to the visible `<h2>`, `aria-describedby` to the consequence paragraph. Focus lands on the least destructive button via `autofocus`. Escape cancels, which maps to the safe outcome. Backdrop click does nothing for permission and destructive dialogs. Focus returns to the invoking element on close. Contains its own local status region (see `ux.md`, section 7). |
| Command palette | Closed, open, empty query, results, no results, loading | Modal `<dialog>` containing a combobox (`role="combobox"`, `aria-expanded`, `aria-controls`, `aria-autocomplete="list"`, `aria-activedescendant`) and a `listbox` of `option` items grouped by `role="group"` with labels. Result count announced through the dialog's local status region, debounced 300 ms. |
| Drawer (sidebar and inspector below 960 px) | Closed, open | Same as dialog (modal), because content behind is hidden. Labelled by its heading. |
| Menu (more actions) | Closed, open | Menu button pattern: `aria-haspopup="menu"`, `aria-expanded`, `menu`, `menuitem`, arrow keys, Escape returns focus to the button. |
| Lightbox (media) | Open, image, video | Native `<dialog>`. Image alt text shown and exposed. Previous and Next buttons, Download, Close. Video uses native controls, never autoplays, no keyboard trap. |

### 7.4 Media gallery items

`<li>` inside `<ul aria-label="Media">`. Each item is a button wrapping the thumbnail (`alt` is the generated description, or "No description" plus the file name and prompt), caption text (title, dimensions, date), and badge for type (Image or Video, text, with icon). Selection mode: checkbox per item with visible labels. Missing alt text is shown visibly as an "Add description" action rather than silently hidden. Video items show duration as text ("Video, 0:12").

## 8. Content and microcopy rules

- Sentence case everywhere. Buttons start with a verb. Errors state what happened, why if known, and what to do next.
- Never use "click": use "select" or "choose" so keyboard and screen reader users are not excluded.
- Numbers and units are written for speech: "2.4 seconds", not "2.4s", in visually hidden text.
- Relative times ("2 minutes ago") are wrapped in `<time datetime>` with absolute time in a tooltip and in the accessible description.
- Truncation: truncate visually with CSS ellipsis but keep the full string in the DOM and in a `title`-equivalent tooltip. Never truncate in the DOM.
- All strings go through an i18n layer; the layout supports RTL via logical properties (`margin-inline`, `padding-block`, `inset-inline-start`). Direction-sensitive icons (`chevron-left`, `chevron-right`, `send`, `undo`) flip in RTL via `transform: scaleX(-1)`.

## 9. Density and responsive rules

| Width | Layout |
|---|---|
| 1200 px and up | Sidebar, conversation column, inspector, optional terminal below the conversation |
| 960 to 1199 px | Sidebar collapses to a rail, inspector is an overlay panel |
| 600 to 959 px | One column, sidebar and inspector are modal drawers |
| below 600 px (including 400 percent zoom of a 1280 px window, which is 320 CSS px) | One column, composer sticky at the bottom, all toolbars wrap, no horizontal page scroll |

Breakpoints are written as container queries on the app shell where possible, so docked panels adapt to their own width rather than the viewport. Code, diff and table areas scroll inside their own focusable, named wrapper, which is the permitted exception to 1.4.10 reflow.

User density setting (Comfortable default, Compact): compact reduces vertical padding by one step but never target size below 24 px.

## 10. Verification before sign-off

- Run an automated check (axe-core or equivalent) on every component story: zero violations. Automated tools find roughly a third of issues, so this is not sufficient on its own.
- Review all four themes plus `forced-colors` emulation (Edge DevTools rendering panel) for every screen.
- Check 200 percent and 400 percent zoom, text spacing overrides, and 150 percent text size.
- Execute the manual screen reader matrix in `ux.md`, section 10.
