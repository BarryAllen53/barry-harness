# Barry icon set

All files live in `frontend/assets/icons/`. Every icon is a 24 by 24 viewBox, stroke-based with `stroke="currentColor"`, `stroke-width="1.75"`, round caps and joins, no fixed colour. Three icons use `fill="currentColor"` for small solid details only (`contrast`, `more`, `logo-mark`). Because everything resolves to `currentColor`, icons follow text colour, hover and disabled states, and the system colours in `forced-colors` mode with no extra rules.

Each file ships with `aria-hidden="true"` and `focusable="false"`. The default assumption is therefore that an icon is decorative and the text next to it, or the accessible name on its button, carries the meaning.

## Rules

1. Decorative icon, with visible text next to it: keep `aria-hidden="true"`. Never add `role="img"` or a `<title>`.
2. Icon-only button: the button gets the name, never the icon. Use a visible `<span class="visually-hidden">` label inside the button, or `aria-label`. The name states the action ("Send message"), not the glyph ("Paper plane").
3. Name text must match any visible tooltip text exactly, so Voice Access ("click Send message") works (WCAG 2.5.3 Label in Name).
4. Toggle buttons keep a constant name and expose state with `aria-pressed` or `aria-expanded`. Do not swap the name from "Show sidebar" to "Hide sidebar" and also set `aria-expanded`.
5. An icon that carries status (check, alert, error, info, shield-check) is always paired with visible status text. Colour and glyph alone never carry meaning. If the text is visually redundant, it is still present as `visually-hidden` text.
6. `play` and `pause` swap only inside a button whose accessible name changes with the action it will perform ("Play clip", "Pause clip"), and no `aria-pressed` is used on that button. Theme icons (`sun`, `moon`, `contrast`) are never swapped: they are fixed glyphs inside a labelled radio group.
7. Sizes: 16 px inside dense chips, 20 px default, 24 px for toolbar buttons, 32 px for empty states. The hit target is at least 24 by 24 CSS px (WCAG 2.5.8) and 44 px for primary touch actions, achieved with padding, not by scaling the glyph.
8. Animation: only `spinner` rotates, and only when `prefers-reduced-motion: no-preference`. In reduced motion it is static and the adjacent text says "Working".
9. Inline the SVG markup in Leptos components (so `currentColor` and the `.icon` class from `tokens.css` apply). Do not load icons through `<img>`, because `currentColor` and forced-colors would not apply.

## Inventory

### Navigation and structure

| File | Meaning | Where used | Accessible-name guidance |
|---|---|---|---|
| `folder.svg` | Project folder | Project tree rows | Decorative. Row text is the project name. |
| `folder-open.svg` | Expanded or active project | Project tree, expanded state | Decorative. State comes from `aria-expanded` on the tree item. |
| `project.svg` | Project overview, workspace | Project header, empty state | Decorative beside "Project". Icon-only: "Project overview". |
| `chat.svg` | Conversation | Chat list, "New chat" | Decorative in lists. Button: "New chat". |
| `sidebar.svg` | Toggle project sidebar | Header | Icon-only button "Projects sidebar" with `aria-expanded` and `aria-controls`. |
| `history.svg` | Past sessions | Sidebar, palette | Decorative beside "History". Icon-only: "Chat history". |
| `command.svg` | Command palette | Header, hint chips | Icon-only button: "Command palette" (shortcut shown in tooltip and `aria-keyshortcuts="Control+K"`). |
| `settings.svg` | Settings | Sidebar footer | Icon-only: "Settings". |
| `keyboard.svg` | Keyboard shortcuts | Settings, help | Decorative beside text, or icon-only "Keyboard shortcuts". |
| `chevron-up.svg` | Collapse, previous | Disclosure, sort | Decorative. Disclosure state is `aria-expanded`. Icon-only previous: "Previous change". |
| `chevron-down.svg` | Expand, next | Disclosure, select | Decorative. Icon-only next: "Next change". |
| `chevron-left.svg` | Back, previous item | Gallery, pagination | Icon-only: "Previous image" or "Back". |
| `chevron-right.svg` | Forward, next item, tree twisty | Gallery, tree | Icon-only: "Next image". As a tree twisty it is decorative. |
| `external-link.svg` | Opens outside the app | Links to docs | Decorative, and the link text ends with the visually hidden "(opens in browser)". |
| `more.svg` | Overflow menu | Rows, cards | Icon-only: "More actions for {item name}". Include the item name so the buttons are distinguishable in a list of buttons. |

### Composer and conversation

| File | Meaning | Where used | Accessible-name guidance |
|---|---|---|---|
| `send.svg` | Send message | Composer | Icon-only: "Send message". Disabled state uses `aria-disabled="true"` with explanation in `aria-describedby` ("Type a message to send"), so it stays discoverable. |
| `stop.svg` | Stop generating | Composer, while generating | Icon-only: "Stop generating". Always in the tab order, always the same position as Send. |
| `upload.svg` | Attach file | Composer | Icon-only: "Attach files". |
| `copy.svg` | Copy | Messages, code blocks | Icon-only: "Copy code" or "Copy response". Result announced through the status region: "Copied". |
| `undo.svg` | Undo, revert change | Diff viewer, file changes | Button: "Revert change to {file}" with confirmation. |
| `refresh.svg` | Retry, regenerate | Message actions, errors | Icon-only: "Regenerate response" or "Retry". |
| `brain.svg` | Reasoning | Reasoning disclosure | Decorative. The disclosure summary text says "Reasoning, 12 seconds". |
| `tool.svg` | Tool call | Tool-call card header | Decorative. Card heading carries the tool name. |
| `plug.svg` | MCP server, connector | Settings, tool cards | Decorative beside "MCP server". Status of connection is text. |
| `key.svg` | API key, credential | Settings, providers | Decorative beside "API key". Never reveal the key in an accessible name. |
| `cost.svg` | Cost and usage | Status bar, message footer | Decorative beside the figure. If the figure is icon-led alone, the container is `aria-label`led: "Session cost: 0.42 dollars". |
| `cache.svg` | Prompt cache hits | Status bar, usage | Decorative beside "Cache". Figures are text. |

### Files, changes and terminal

| File | Meaning | Where used | Accessible-name guidance |
|---|---|---|---|
| `file.svg` | File | File lists, attachments | Decorative. Name is the file name. |
| `file-edit.svg` | Edited file | Tool cards, changes list | Decorative. Visible text: "Edited" plus path. |
| `diff-add.svg` | Added lines | Diff headers, counts | Decorative. Always followed by text: "12 added". |
| `diff-remove.svg` | Removed lines | Diff headers, counts | Decorative. Always followed by text: "3 removed". |
| `branch.svg` | Git branch | Status bar, project header | Decorative beside branch name, or the group is labelled "Git branch: main". |
| `terminal.svg` | Terminal, command | Terminal pane, tool cards | Decorative beside "Terminal". Icon-only toggle: "Terminal panel" with `aria-expanded`. |

### Safety and status

| File | Meaning | Where used | Accessible-name guidance |
|---|---|---|---|
| `shield.svg` | Permission needed, policy | Permission dialog, settings | Decorative. Heading text carries the meaning. |
| `shield-check.svg` | Allowed, trusted | Permission history, settings | Decorative with text "Allowed". |
| `alert.svg` | Warning, high impact | Banners, permission dialog | Decorative with text "Warning" or "High impact", never alone. |
| `error.svg` | Failure | Tool cards, banners | Decorative with text "Failed". |
| `info.svg` | Information | Banners, hints | Decorative with text "Note". |
| `check.svg` | Success, selected | Status chips, menu items, copied | Decorative with text "Succeeded". In menus, selection uses `aria-checked`. |
| `close.svg` | Close, dismiss | Dialogs, panels, chips | Icon-only: "Close", "Close inspector" or "Dismiss notice". |
| `spinner.svg` | Working | Running chips, loading | Decorative. Adjacent text "Running" or, in a skeleton, the status region announces "Loading projects". |
| `plus.svg` | Add, new | Add project, add server | Icon-only with specific noun: "Add project". |
| `minus.svg` | Remove from list, collapse | Steppers, lists | Icon-only: "Remove {item}", "Decrease font size". |
| `trash.svg` | Delete | Chat and project rows | Icon-only: "Delete {item}". Always leads to a confirmation dialog whose least destructive button is focused by default. |

### Media and search

| File | Meaning | Where used | Accessible-name guidance |
|---|---|---|---|
| `search.svg` | Search | Palette input, sidebar filter | Decorative inside a labelled input. Icon-only button: "Search". |
| `image.svg` | Image | Gallery filter, attachments | Decorative. Filter button text: "Images". |
| `video.svg` | Video | Gallery filter, attachments | Decorative. Filter button text: "Videos". |
| `play.svg` | Play | Video and audio controls | Icon-only: "Play {title}". Native controls are preferred. |
| `pause.svg` | Pause | Video controls | Icon-only: "Pause {title}". |
| `download.svg` | Save | Gallery items, exports | Icon-only: "Download {file name}". Completion announced via status. |

### Appearance

| File | Meaning | Where used | Accessible-name guidance |
|---|---|---|---|
| `sun.svg` | Light theme | Theme radio group | Decorative inside a radio labelled "Light". |
| `moon.svg` | Dark theme | Theme radio group | Decorative inside a radio labelled "Dark". |
| `contrast.svg` | High contrast theme | Theme radio group | Decorative inside a radio labelled "High contrast". |

### Brand

| File | Meaning | Where used | Accessible-name guidance |
|---|---|---|---|
| `logo-mark.svg` | Barry bear mark | Header, empty states, about, window icon source | In the header it sits next to the visible word "Barry" and is decorative. When it is the only content of a link, the link name is "Barry, home". Minimum size 16 px, clear space equal to one ear radius (2.8 units of the 24 grid). |

## Name-writing rules

- Verb plus noun, sentence case, no trailing punctuation: "Delete chat", not "Trash" or "Delete chat?".
- Name the target when several identical buttons appear in one list: "Delete chat: Fix login bug".
- Do not include the role ("button") or the state in the name.
- Do not use the word "icon".
- Do not duplicate nearby visible text: if a heading says "Terminal", the toggle inside it is "Show terminal" or uses `aria-expanded`, not "Terminal terminal".
- Localisation: names are strings, never derived from file names.

## Review checklist for a new icon

- 24 by 24 viewBox, 1.75 stroke, round caps and joins, content inside a 3 to 21 unit safe area.
- `currentColor` only, no hard-coded colours, no gradients, no comments.
- Reads at 16 px: no detail thinner than 1 px at that size.
- Still distinguishable when text is removed in `forced-colors` mode.
- Has a row in this file with accessible-name guidance.
