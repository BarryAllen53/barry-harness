# Barry UX specification

Status: design specification for the Leptos 0.8 CSR frontend hosted in Tauri 2 (WebView2) on Windows 11. Visual tokens and components: `design-system.md`. Icons: `icons.md`.

Accessibility status: every screen reader behaviour in this document is a design hypothesis built on documented behaviour of NVDA, JAWS and Narrator with Chromium-based WebView2. None of it has been verified on real assistive technology yet. Section 10 is the manual test matrix that turns hypotheses into facts. Where two designs are viable, the document names the decision test that picks one.

## 1. Principles

1. One conversation, one reading order. DOM order equals visual order equals tab order.
2. Streaming is for eyes, completion is for ears. Sighted users watch text arrive. Screen reader users are told what Barry is doing in short status messages and receive the answer once, whole, when it is complete.
3. Nothing important lives only in colour, motion, position or hover.
4. Focus stays where the user is working. The composer keeps focus while the agent works. Focus moves only when the user caused it, or when a modal dialog demands a decision.
5. Destructive and permission decisions default to the safest option and explain their consequences in text.
6. Every feature is reachable from the keyboard and from the command palette.
7. Never trap, never auto-play, never time out on a decision.
8. Keyboard shortcuts never rely on single letters outside text fields (screen readers use them for browse mode), never on Insert or Caps Lock, never on Ctrl+Alt (AltGr on many keyboard layouts, including Turkish, and the table navigation keys of NVDA and JAWS).

## 2. Information architecture

```
Barry
  Projects (folders on disk)
    Chats (sessions) per project
      Messages
        Reasoning (per assistant message)
        Tool calls (per assistant message)
          Arguments and result
          Diff (file edits)
          Terminal output (commands)
          Permission requests (resolved in a dialog, logged in the card)
        Media outputs (images, videos)
    Project files and changes (inspector: Changes tab)
    Project media (gallery)
  Settings
    General, Appearance, Accessibility, Providers and keys, Models,
    Permissions, MCP servers, Shortcuts, Data and privacy
  Command palette (global, searches all of the above plus commands)
```

Primary views (one at a time in the main area, switched by the sidebar, palette or shortcut):

| View | Purpose | Heading (h1) |
|---|---|---|
| Chat | Conversation with the agent in the current project | Chat title, for example "Fix login redirect" |
| Gallery | All images and videos in the project | "Media" |
| Settings | Preferences | "Settings" |
| Welcome | No project open | "Open a project to begin" |

Secondary surfaces: inspector (side panel with tabs Reasoning, Changes, Files), terminal pane (bottom), command palette, permission dialog, confirmation dialogs, shortcut reference dialog.

The window title mirrors context and is updated on every view change: "{Chat title} - {Project name} - Barry". Screen readers announce the title change on focus return, and it is what appears in Alt+Tab.

## 3. Screen layouts

Wide layout (1200 px and up). `S` sidebar, `M` main, `I` inspector, `T` terminal.

```
+--------------------------------------------------------------------------+
| header: [sidebar toggle] Barry mark  Project / Chat title   [palette] [?] |
+-----------+------------------------------------------+-------------------+
| S         | M  Chat title (h1)                       | I  Inspector (h2) |
| Projects  |  +------------------------------------+  | [Reasoning|Changes|Files]
|  v App A  |  | You: message                       |  |                   |
|    chat 1 |  | Barry: reasoning (disclosure)      |  |  tab panel        |
|    chat 2 |  |   tool calls (cards)               |  |                   |
|  > App B  |  |   answer                           |  |                   |
|           |  +------------------------------------+  |                   |
| [new proj]|  | composer: [ message box ]  [Send]  |  |                   |
| [settings]|  +------------------------------------+  |                   |
+-----------+-------------------[ T terminal pane ]---+-------------------+
| footer: model, branch, tokens, cache, cost, connection                    |
+--------------------------------------------------------------------------+
```

Narrow layout (below 960 px and 400 percent zoom): a single column. Sidebar and inspector open as modal drawers from header buttons. The composer is sticky at the bottom. The terminal pane becomes a tab in the inspector drawer.

### 3.1 Project sidebar

- `<nav aria-label="Projects">` containing a heading `<h2>Projects</h2>` (visible), a filter field (`type=search`, label "Filter projects and chats"), the tree, and footer buttons "Add project" and "Settings".
- Tree: project rows (folder icon, name, branch chip when in git) with children chats. Each chat row shows title, relative time, and a status dot paired with text for "Generating" or "Needs permission" (text in a visually hidden span, plus a visible chip of at least 24 px).
- Row actions (rename, delete, more) live behind a "More actions for {name}" button that is part of the row, not hover-only. They appear on row hover and on row focus, and exist in the accessibility tree always.
- States: loading (skeleton plus status "Loading projects"), empty ("No projects yet" with an "Add project" button), error ("Could not read the projects folder" with "Retry"), project folder missing (chip "Folder not found", row remains and offers "Locate folder").
- Collapsed rail (56 px): icons only, each a button with a name; tooltip on focus.

### 3.2 Transcript

- `<div role="log" aria-live="off" aria-label="Conversation: {chat title}" tabindex="-1">` inside `<main>`. The wrapper is the scroll container and a focus target for the "go to conversation" shortcut.
- Inside: a `<ol>` of messages, each `<li>` containing an `<article>`.
- Message layout: visible sender label ("You", "Barry") with the bear mark for Barry, timestamp in `<time>`, body, then footer actions (Copy, Regenerate, and Edit for user messages).
- User messages sit on a `--color-sunken` surface with a border; Barry messages sit directly on the page with a left rule. The sender label, not the surface, is the identity cue.
- Streaming: the in-progress assistant message grows visibly. Its `<article>` has `aria-busy="true"`. A stable, visible "Generating" chip with `spinner.svg` sits above the body and a Stop button is in the composer.
- "Jump to latest" button appears when the user has scrolled away from the bottom while streaming. It is a real button in the tab order after the transcript.
- Long content: code blocks and tables scroll inside their own named, focusable wrappers. Long unbroken strings wrap (`overflow-wrap: anywhere`).
- Empty chat: heading "What should we work on?", three suggested prompts as buttons, and the folder path.
- Error message state: a failed turn shows an inline error block (icon, "Request failed", cause, "Retry" button), part of the article.
- Stopped state: "Stopped by you" chip, partial answer preserved and inserted as complete text.

### 3.3 Composer

- A `<form aria-label="Send a message">` pinned to the bottom of main: textarea (visually hidden label "Message to Barry", visible hint "Enter to send, Shift+Enter for a new line"), toolbar with Attach files, model chip (opens a model picker), and the Send or Stop button.
- Send and Stop occupy the same position. While generating, Stop is shown and Send is hidden through DOM replacement, not both present. Both have fixed labels: "Send message", "Stop generating". The swap is announced by the status region ("Generating response"), not by moving focus.
- The textarea stays enabled while generating so the user can draft the next message. Sending while generating is queued, and the queue is shown as a chip "1 message queued" with a "Cancel queued message" button.
- Attached files show as a list of chips with a remove button each. Count and names are in the form's `aria-describedby` text.
- States: empty (Send `aria-disabled` with description "Type a message to send"), typing, over limit (inline error tied through `aria-describedby`), offline (textarea remains editable, Send disabled with description "Offline. Barry cannot send right now."), generating, permission pending (composer remains usable but a banner says "Waiting for your answer"; the permission dialog is already open).
- Focus: Escape inside the composer never clears text and never stops generation.

### 3.4 Reasoning panel

Two presentations of the same data:

- Inline: a `<details>` inside the assistant message titled "Reasoning" with the elapsed time, closed by default once complete and open while streaming for sighted users (a visual state only, see section 8.1).
- Inspector tab "Reasoning": a reading view of reasoning for the selected message, with the message heading as context. It is a static document updated on completion, a plain `<section>` with headings and paragraphs.
- Reasoning is shown as plain text in a quieter style (`--color-text-muted`, smaller measure), with an icon `brain.svg` decorative. A setting "Show reasoning" has three values: Hidden, Collapsed (default), Expanded.
- Screen reader users never hear reasoning while it streams. After completion the summary reads "Reasoning, 12 seconds, collapsed".

### 3.5 Tool-call cards

- A list inside the assistant message: `<ul aria-label="Tool calls">` with one `<li>` per call.
- Card header (h4): `tool.svg` (or `plug.svg` for MCP), tool name, target in `--font-mono` (for example `src/main.rs`), status chip, duration, expand control.
- Status chip vocabulary (text, always present): Waiting for permission, Running, Succeeded, Failed, Denied, Cancelled. Icons: `shield`, `spinner`, `check`, `error`, `shield` (with "Denied" text), `close`.
- Body (inside `<details>`): arguments as a definition list, then result: diff (file edit), terminal output (command), or text. Large results are truncated with "Show all 240 lines".
- Keyboard: Tab to the summary, Enter or Space toggles. No custom key handling.
- Cards that need attention (permission pending, failed) carry an explicit leading `alert` or `shield` icon and text and are first in the Tab order among the message's controls.

### 3.6 Diff viewer

- Appears in the tool card for an edit and in the inspector "Changes" tab, which aggregates all edits of the chat per file.
- Header per file: `file-edit.svg`, path, counts "12 added, 3 removed" (with `diff-add` and `diff-remove` icons and text), and buttons "Copy patch", "Revert changes" (confirmation dialog), and a collapse toggle.
- View modes: Unified (default, the only mode exposed as a table), Side by side (visual only for sighted users, with the left side `aria-hidden` removed; hidden when a screen reader optimised setting is on, see Settings), and Summary list ("Line 14 removed: ...").
- Unified table structure: `<table>` with `<caption>` ("Changes to src/main.rs: 12 added, 3 removed"), column headers Old line, New line, Change, Code. Hunk header rows use `<th scope="colgroup" colspan="4">` with text "Hunk 2: lines 40 to 58". In changed rows the Change cell contains a visible glyph (`+` or `-`, `aria-hidden`) and visually hidden "Added:" or "Removed:" before the code. Unchanged context rows are plain and their Change cell is empty, unless the user chose "Announce unchanged lines" in Settings.
- Added and removed rows use the diff tokens, plus the gutter glyph, so the diff is readable in forced colours and greyscale.
- Intra-line changes use bold plus underline for added and strikethrough for removed, not colour alone.
- Navigation: F8 and Shift+F8 move focus to the next and previous hunk header (header rows are focusable with `tabindex="-1"` and receive programmatic focus). Tab exits the table to the next control. The table wrapper is `role="region"` with `aria-label="Diff of src/main.rs"` and `tabindex="0"` so keyboard users can scroll it horizontally.
- States: loading, binary file ("Binary file changed, 24 KB to 31 KB"), too large ("Large diff, 4,200 lines. Show anyway"), empty ("No differences"), conflict, reverted.

### 3.7 Terminal pane

- A bottom `<section aria-label="Terminal output">` (region landmark), collapsed by default, toggled by Ctrl+J and a footer button with `aria-expanded`.
- v1 is an output viewer for commands the agent runs, not an interactive shell.
- Structure: a list of command runs. Each run is an `<article>` with heading "Command: cargo test" (h3), status text "Running", "Finished, exit code 0" or "Failed, exit code 101", duration, and a `<pre tabindex="0" role="region" aria-label="Output of cargo test">`.
- While running: visual output scrolls live for sighted users. It is `aria-hidden="true"` and a visually hidden line reads "Running. Output will be available when the command finishes." On completion the live visual is replaced by the final text (ANSI stripped into plain text, `aria-hidden` removed), and the status region announces "Command finished, exit code 0, 42 lines of output".
- Provide a "Copy output" button and "Open full output" for very large logs (more than 2,000 lines are virtualised visually, but the complete plain text is available via "Open full output" as a single `<pre>` in a dialog or the Files tab).
- Stderr lines are prefixed with visually hidden "Error output:" only in the first line of each stderr block, to avoid chatter.
- States: idle ("No commands yet"), running, success, failure, cancelled, truncated.

### 3.8 Permission dialog

A native `<dialog>` opened with `showModal()` whenever Barry needs approval. Structure:

```
+---------------------------------------------------------+
| [shield] Allow Barry to run a command?          (h2)     |
| 1 of 2 requests                                          |
|---------------------------------------------------------|
| Barry wants to run this command in D:\Projects\App:      |
|   cargo test --workspace                       (code)    |
|                                                          |
| Tool: Shell    Directory: D:\Projects\App    Risk: Low   |
| What can happen: The command can read and change files   |
| in this project. It cannot reach outside the folder.     |
|---------------------------------------------------------|
| [ Deny ]   [ Allow once ]   [ Allow for this chat ]      |
+---------------------------------------------------------+
```

- `aria-labelledby` points to the h2. `aria-describedby` points to the "What can happen" paragraph (the consequence). The command text is inside a `<pre tabindex="0">` labelled "Command", reachable by Tab right after the title text, before the buttons.
- Focus on open: the Deny button (`autofocus`), the least destructive choice. Enter on open therefore denies. Escape also denies, with the reason "Denied by you".
- The button order is the DOM order and the visual order: Deny, Allow once, Allow for this chat. The most permissive option is last and visually secondary (outline, not filled). "Allow once" is the filled accent button but not the default focus.
- High-risk requests (delete outside project, network access, writes to system paths, secrets in arguments): the icon becomes `alert`, the title is prefixed "High impact:", the consequence paragraph is explicit, "Allow for this chat" is not offered, and a required checkbox "I understand this can delete files" appears before Allow once becomes enabled. The checkbox is labelled and the disabled button explains itself via `aria-describedby`.
- Never auto-dismiss, no timeout. Backdrop click does nothing.
- Multiple pending requests queue: the title shows "1 of 3 requests", the dialog stays open and advances to the next one without closing (the heading text change is announced through the dialog's local status region, and focus returns to Deny on the new request).
- When the dialog closes, focus returns to the composer if it launched from background work, or to the invoking control otherwise.
- The tool-call card records the outcome as text ("Denied by you", "Allowed once").

### 3.9 Media gallery

- View with `<h1>Media</h1>`, a filter group (All, Images, Videos as a radio group styled as segmented buttons, labelled "Show"), a sort select, and a search field.
- `<ul aria-label="Media items">` with a responsive grid (`repeat(auto-fill, minmax(12rem, 1fr))`). Each `<li>` holds a button "Open {title}, {type}, {dimensions or duration}" wrapping the thumbnail (alt text is the generated description, or "No description available" with a visible "Add description" button).
- Grid keyboard: Tab moves item to item (a flat list, no roving grid, so screen reader navigation works the same way as in any list). Optional arrow-key navigation is added later only if testing shows long lists are tedious.
- Lightbox: native `<dialog>` with title (file name), media, description textarea for editing alt text, Previous, Next, Download, Delete, Close. Images: alt text exposed on the image and shown as a caption. Videos: native `<video controls>` with captions track when available, no autoplay, `preload="metadata"`, a visible "Transcript or description" disclosure. Left and Right move between items only when focus is on Previous or Next, so they do not clash with the video seek keys.
- Count text "24 items" with `aria-live` off; filter changes are announced by the status region ("12 images shown").
- States: loading skeleton, empty ("No media yet. Images and videos Barry creates will appear here"), error, partial (some thumbnails failed, shows an icon and text "Preview unavailable" and still lists file name), large library (virtualised by rows with the list size known through `aria-setsize` and `aria-posinset`).

### 3.10 Settings

- View with `<h1>Settings</h1>`. Left a list of sections (`<nav aria-label="Settings sections">` with links), right a form. Each section is a `<section aria-labelledby>` with an `<h2>`. In narrow layout the section list becomes a select-like disclosure at the top.
- Controls are native: radio groups, switches, selects, number inputs, with visible labels and hints tied by `aria-describedby`. Save is automatic and each change announces "Saved" through the status region (debounced, one message per burst). There is no unsaved state, so no discard button.
- Sections: General (language, startup behaviour), Appearance (theme radio group: System, Light, Dark, High contrast; text size 87.5 to 150 percent; density), Accessibility (see below), Providers and keys (API key fields masked, "Test connection" button), Models, Permissions (default policy per tool: Ask, Allow, Deny; list of saved allow rules with "Remove" buttons), MCP servers (list, add, status text), Shortcuts (list of commands with current key, "Change" button opens a capture dialog, conflict warnings include "This combination is used by screen readers" for flagged combinations), Data and privacy (history retention, clear data with confirmation, cost and cache budget limits).
- Accessibility section: Announcement verbosity (Minimal, Standard, Verbose), "Read full response when complete" (off by default, limited to responses under 1,200 characters), "Announce long-running progress" (30 second, then every 60 second), "Announce unchanged diff lines", "Screen reader optimised layout" (disables side-by-side diff, collapses decorative chrome, expands reasoning only when requested), Enter key behaviour (Enter sends, or Ctrl+Enter sends), Reduce motion (follows system, always, never), "Sound cues" (off by default, short earcons for completion and permission).
- Secrets: API key fields never reveal values to assistive technology while masked; the field description says "Key saved, ending in 4F2A".

### 3.11 Command palette

- Opened by Ctrl+K or Ctrl+Shift+P. A modal `<dialog aria-label="Command palette">` with a combobox input and a result listbox. Max 8 visible options, scrolling beyond.
- Content: commands (New chat, Open project, Toggle sidebar, Stop generating, Switch theme, Open settings, Show keyboard shortcuts, Copy last response, Go to message), projects and chats (fuzzy search), files in the project (prefixed "File:"), recent media.
- Each option: icon (decorative), label, secondary text (scope), and the shortcut in `<kbd>`. Option text is the accessible name; `aria-keyshortcuts` is set where relevant. Groups are labelled (`role="group"` with `aria-labelledby`).
- Prefix modes: `>` commands only, `@` chats, `#` files. These are characters typed in the text field, so there is no conflict with browse mode.
- Result count is written into the palette's own local status region ("6 results") after a 300 ms debounce. Not announced when the count stays the same.
- States: empty query (recent items and top commands), results, no results ("No matches for {query}"), loading (file index warming up, "Searching files" in the status region).
- Escape closes. Enter runs the highlighted option. If a command needs an argument (rename), the palette closes and the relevant control gets focus.

## 4. Landmarks and heading structure

Landmark map (one of each unless stated):

| Landmark | Element and name | Contents |
|---|---|---|
| banner | `<header>` | App mark, sidebar toggle, project and chat breadcrumb, command palette button, help button |
| navigation | `<nav aria-label="Projects">` | Project and chat tree, add project, settings link |
| main | `<main id="main">` | View content (transcript and composer, gallery or settings). Exactly one `<h1>`. |
| complementary | `<aside aria-label="Inspector">` | Reasoning, Changes, Files tabs |
| region | `<section aria-label="Terminal output">` | Terminal runs |
| contentinfo | `<footer>` | Model, branch, tokens, cache, cost, connection (text) |
| search | `<form role="search">` inside the sidebar filter | Filter projects and chats |
| form | composer `<form aria-label="Send a message">` | Composer controls |

The composer is inside `<main>`, after the transcript. Dialogs are outside the landmark structure by nature (top layer).

Heading outline for the chat view:

```
h1  Fix login redirect                    (chat title, tabindex=-1 for focus on route change)
  h2  Conversation                         (visually hidden, labels the log)
    h3  You, 14:32                         (visually hidden, inside each user article)
    h3  Barry, 14:33                       (visually hidden, inside each assistant article)
      h4  Reasoning                        (summary text acts as the label, no heading needed)
      h4  Tool call: Edit file src/main.rs
  h2  Message                              (visually hidden, composer form)
h2  Projects                               (visible, in the sidebar nav, which is before main in DOM)
h2  Inspector                              (visible, aside)
  h3  Reasoning | Changes | Files          (tab panel headings, visually hidden when tabs are present)
h2  Terminal output                        (visible, region)
  h3  Command: cargo test
```

Rules: heading levels never skip. The sidebar `h2` appears in the DOM before `main`'s `h1`, which is acceptable because navigation precedes main content; an `h1` for the application is not needed because the title and banner carry it. In Settings: `h1` Settings, `h2` per section. In dialogs: `h2` as the dialog title. In the gallery: `h1` Media, items have no headings (they are list items), lightbox title is `h2`.

Route changes (switching chat, opening Settings or Gallery): the window title updates, focus moves to the new view's `h1` (`tabindex="-1"`), and the status region announces "{View name} opened" once, only when focus did not move due to the user activating a link inside the same view. This pattern replaces page-load announcements that a CSR app lacks.

## 5. Focus order and focus management

### 5.1 Tab order (wide layout)

1. Skip links: "Skip to conversation", "Skip to message box" (visible on focus).
2. Header: sidebar toggle, breadcrumb links, command palette button, help.
3. Sidebar: filter field, tree (one tab stop, arrows inside), Add project, Settings.
4. Main: chat title (not focusable except programmatically), transcript scroll container (`tabindex="-1"`, not a tab stop), interactive elements inside messages in document order (message actions, tool card summaries, code copy buttons, links), "Jump to latest" when visible.
5. Composer: textarea, Attach files, model picker, Send or Stop.
6. Inspector: tabs (one tab stop, arrows inside), active panel content.
7. Terminal pane (when open): toggle, run outputs.
8. Footer: items with interactive content only (model picker is shared with composer, so the footer offers none that duplicate).

Positive `tabindex` is never used. The transcript scrolling region itself is not in the tab order, but its content is reachable, and every scrollable code or table wrapper inside it is (`tabindex="0"`) so that keyboard users can scroll them.

### 5.2 Focus rules

| Event | Focus behaviour |
|---|---|
| App start with project | Focus on the composer. Window title set. |
| App start without project | Focus on the Welcome h1, then "Open project" button is the first tab stop in main. |
| User sends a message | Focus stays in the composer. Textarea is cleared. Status: "Generating response". |
| Generation starts, ends or fails | Focus does not move. Status region announces it. |
| Final answer inserted | Focus does not move. The user can move to it with Alt+Down (focuses the latest message article) or with their screen reader's article or heading navigation. |
| Stop pressed | Focus stays on the Stop button's replacement: because the button swaps to Send in the same position, focus is placed on the composer textarea programmatically, which is the logical next action, and the status says "Stopped". |
| Permission requested | `showModal()`. Focus to Deny. On close, focus returns to the element focused before (usually the composer). |
| Dialog closes | Focus returns to the invoker. If the invoker no longer exists (deleted item), focus goes to the nearest logical element, for example the next row in the tree, never to `body`. |
| Command palette | On open, input focused. On close, focus returns to the previous focus. If a command navigates, the new target gets focus. |
| Switch view or chat | Focus moves to the new `h1`, then Tab leads to the first control. |
| Delete chat | After confirmation, focus goes to the next row in the tree, or the previous if it was last, or "Add project" if the list is empty. Status: "Chat deleted". |
| Inline expand (disclosure) | Focus stays on the summary. |
| Inspector tab change | Focus stays on the tab, panel updates. |
| Drawer opens (narrow) | Treated as a dialog: focus into the drawer heading, close returns to the toggle. |
| Error banner appears | Focus does not move, status announces. If the error blocks sending, the banner links to the retry action and is placed immediately before the composer. |

Focus indicator: 3 px `--color-focus` outline with a 2 px offset on every focusable element, never removed, verified in all themes (see contrast table in `design-system.md`). Forced colours use `Highlight`. Focus is never obscured by sticky elements: scroll containers use `scroll-padding-block` equal to the sticky header and composer heights (WCAG 2.4.11).

## 6. Keyboard shortcut map

### 6.1 Design constraints

- Screen reader keys to avoid: NVDA, JAWS and Narrator all use Insert and Caps Lock as the screen reader modifier. Never bind Insert or Caps Lock chords. Narrator also uses Caps Lock+letter (for example Caps Lock+Space for scan mode toggle). JAWS uses Insert+letter and Insert+number-pad.
- Browse and scan mode keys to avoid as global shortcuts: unmodified letters, digits and symbols (H, 1 to 6, D, F, K, L, T, B, E, G, X, Q, S, M, N, P, O, V, C, I, U, R, A, Z, and Space, Enter, Escape behave specially). Shortcuts either need a modifier or work only while focus is inside a specific widget (arrow keys in a tree, for instance).
- Avoid Ctrl+Alt+anything and Alt+Ctrl+Shift: AltGr equals Ctrl+Alt on Turkish, German, French and other layouts, and typing characters such as `@`, `{`, `[`, `\` would trigger commands. Ctrl+Alt+Arrow also moves between table cells in NVDA and JAWS.
- Avoid Alt+Shift and Ctrl+Shift alone (keyboard layout switching) as a complete combination. A letter added to them is safe.
- Avoid Windows reserved keys: Win combos, Alt+Tab, Alt+F4, Alt+Space, Ctrl+Shift+Esc, Ctrl+Esc, F10 (menu bar mode and Shift+F10 is the context menu key), F11.
- Avoid shortcuts Windows and WebView2 use that users expect: Ctrl+C, X, V, A, Z, Y, F, P, R, F5, F12, Ctrl+Plus and Minus (zoom).
- Use keys that exist on a Turkish Q layout without AltGr: letters, digits, comma, period, Enter, arrows, F-keys. Avoid `/`, `\`, `[`, `]`, backtick, `<`, `>` as shortcut keys, because they need Shift or AltGr there.
- Every shortcut is also a palette command, a visible control, and is rebindable in Settings. Rebinding warns about reserved and flagged combinations.
- `aria-keyshortcuts` is set on the controls that have them (for example `Control+K` on the palette button), and shortcuts are listed in tooltips and in the shortcut dialog (F1).

### 6.2 Global shortcuts

| Action | Shortcut | Notes |
|---|---|---|
| Command palette | Ctrl+K, Ctrl+Shift+P | Alias for people used to editors |
| Keyboard shortcut reference | F1 | Dialog listing everything, searchable |
| Focus projects sidebar | Ctrl+1 | Moves focus to the tree (current item) |
| Focus conversation | Ctrl+2 | Focuses the transcript container |
| Focus message box | Ctrl+3 | Focuses the composer textarea |
| Focus inspector | Ctrl+4 | Focuses the active inspector tab |
| Focus terminal pane | Ctrl+5 | Opens it if closed |
| Next region, previous region | F6, Shift+F6 | Cycles: sidebar, conversation, composer, inspector, terminal, footer |
| Toggle projects sidebar | Ctrl+B | Focus stays or moves to the toggle's logical target |
| Toggle inspector | Ctrl+I | |
| Toggle terminal pane | Ctrl+J | |
| Open media gallery | Ctrl+M | |
| Settings | Ctrl+, | |
| New chat in this project | Ctrl+N | |
| Open project folder | Ctrl+O | Native folder picker |
| Stop generating | Ctrl+. | Same effect as the Stop button |
| Previous message, next message | Alt+Up, Alt+Down | Focus moves to the adjacent message article (`tabindex="-1"`). From the composer, Alt+Up goes to the latest message. |
| Go to latest message | Ctrl+End when focus is in the transcript | Standard scroll-to-end |
| Copy last response | Palette command only | Avoids Ctrl+Shift+C devtools conflicts |
| Close top-most overlay | Escape | One layer at a time. Never stops generation. |

### 6.3 Composer

| Action | Shortcut |
|---|---|
| Send | Enter (default setting) or Ctrl+Enter, per Settings |
| New line | Shift+Enter (or Enter when Ctrl+Enter sends) |
| Recall previous message | Alt+Up is reserved for messages, so recall uses Up Arrow only when the textarea is empty |
| Attach files | Palette command and the Attach button |

Enter is ignored while an IME composition is active (`isComposing`).

### 6.4 Widgets (active only when focus is inside)

| Widget | Keys |
|---|---|
| Project tree | Up, Down move. Right expands or enters group. Left collapses or goes to parent. Home, End. Enter opens. F2 renames. Delete opens the delete confirmation. Type-ahead selects by first letters |
| Tabs | Left, Right (automatic activation), Home, End |
| Disclosure | Enter, Space |
| Diff | F8 next hunk, Shift+F8 previous hunk. Tab leaves the table |
| Command palette | Up, Down, Home, End, Enter, Escape, Tab closes without running (focus returns to invoker) |
| Menu | Up, Down, Home, End, Escape, Enter |
| Lightbox | Escape closes, Previous or Next buttons with Enter. Space toggles play or pause on a focused video, with native controls |
| Dialog | Tab and Shift+Tab cycle inside, Escape cancels (safe outcome) |
| Shortcut capture (Settings) | Press the new combination. Escape cancels, Backspace clears. The dialog states how to cancel before capturing |

### 6.5 Known collisions checked

| Shortcut | Possible conflict | Resolution |
|---|---|---|
| Ctrl+K | Browsers focus the address bar search. Not present in Tauri. JAWS has no Ctrl+K. | Keep |
| Ctrl+J | Downloads page in browsers. Not present in Tauri. | Keep |
| Ctrl+I | Italic in rich editors. The composer is plain text. | Keep |
| Ctrl+B | Bold. Plain text. | Keep |
| Ctrl+M | Narrator and NVDA do not bind it by default. | Keep, test case KB-01 |
| Ctrl+N, Ctrl+O | Browser new window and open. Intercepted in the app. | Keep |
| Ctrl+, | None | Keep |
| Ctrl+. | Windows emoji panel is Win+. , not Ctrl+. | Keep |
| F6 | Browser focus cycling. Intercepted in the app. | Keep, with the real panes cycling |
| F8 | JAWS has no default F8 in applications. NVDA none. | Keep, test case KB-01 |
| F1 | Help in Windows apps. Intercepted. | Keep |
| Alt+Up, Alt+Down | Browser back is Alt+Left. | Keep, test case KB-01 |
| Ctrl+1 to Ctrl+5 | Tab switching in browsers. No tabs in the app. | Keep |

Test case KB-01 in section 10 runs every shortcut under NVDA, JAWS and Narrator in both focus mode and browse or scan mode to confirm that the app receives it and the screen reader does not intercept it.

## 7. Live-region and announcement policy

### 7.1 Architecture

- Transcript: `role="log"` with `aria-live="off"` and an accessible name. It exists so users can navigate it, not so it speaks.
- Announcer: one always-present, visually hidden `<div role="status" aria-live="polite" aria-atomic="true">` at the root of the app, rendered at first paint and never removed or re-created. It is the only polite region in the application.
- Local announcers: each modal `<dialog>` (permission, confirmation, palette, lightbox, drawer) contains its own `role="status"` element, because opening a modal makes everything outside it inert, and live regions in inert content are not exposed by Chromium to assistive technology. Messages for events that happen while a dialog is open are routed to that dialog's local region, and the root announcer is silent until the dialog closes (then only the final state is announced).
- No other `aria-live`, `role="alert"`, or `role="status"` exists. Visual toasts, banners and chips are not live regions.
- Streaming text, reasoning text, terminal output and tool arguments never enter any live region.
- Announcer service (Leptos context): `announce(Priority, message)` with a queue. Rules: updates within a 400 ms window are coalesced into the latest one of the same category, a minimum of 1.5 s separates announcements except terminal states (complete, stopped, failed, permission) which replace queued items, and identical consecutive messages gain a suffix such as "(2)" so they are re-announced. The region text is set in a single DOM write, never cleared and rewritten in the same frame.
- Verbosity settings: Minimal (completion, failure and permission only), Standard (the table below), Verbose (adds tool starts and results and elapsed-time updates).

### 7.2 Policy table

| Event | Announced text (example) | Region | Debounce and priority | Standard | Notes |
|---|---|---|---|---|---|
| Message sent, generation starts | "Generating response" | Root status, polite | 400 ms. Replaces earlier | Yes | Once per turn. Not repeated on every token. |
| Reasoning in progress | none | none | n/a | No | Visual only, the reasoning panel shows an elapsed timer that is `aria-hidden`. |
| Tool starts | "Running tool: Edit file src/main.rs" | Root status, polite | Coalesced. At most one per 1.5 s, later tools replaced by "Running 3 tools" | Verbose only. In Standard, folded into "Generating response" until completion | Avoids a barrage on agent loops. |
| Tool needs permission | "Permission needed: Run command" | Root status only when no modal can open. The dialog itself announces through focus and its heading and description | n/a | Yes | Do not announce twice: the dialog's focus announcement is the notification. The root announcer is used only as a fallback when the app window is not focused (message "Barry is waiting for permission"). |
| Permission answered | "Allowed once: Run command" or "Denied: Run command" | Dialog-local status just before close, otherwise root | Immediate | Yes | |
| Tool finished | "Edit file succeeded" or "Command failed, exit code 1" | Root, polite | Coalesced with others | Failures yes, success only in Verbose | |
| Command finished (terminal) | "Command finished, exit code 0, 42 lines of output" | Root, polite | Immediate | Yes | Text transcript becomes available at this moment. |
| Response complete | "Response complete, 214 words" | Root, polite | Immediate. Replaces queued | Yes | Final answer is inserted into the transcript first, then the status updates in the next frame. |
| Response complete, read aloud option | "Barry says: {first 1,200 characters}" | Root, polite | After "Response complete" | Opt-in | Only below 1,200 characters, otherwise "Response complete, 640 words. Press Alt+Up to read it." |
| Generation stopped by user | "Stopped. Partial response kept" | Root, polite | Immediate | Yes | |
| Error (network, provider, tool crash) | "Error: could not reach the provider. Retry is available." | Root, polite | Immediate. Replaces queued | Yes | Decision D-1: if tests show polite errors are lost, add a separate always-present `role="alert"` region used for errors only. No other use. |
| Long-running progress | "Still working, 30 seconds" then each 60 s | Root, polite | Timer | Opt-in | Off by default to avoid constant interruption. |
| Offline, back online | "Offline. Barry cannot send right now." / "Back online." | Root, polite | 2 s debounce to avoid flapping | Yes | |
| Copy to clipboard | "Copied" | Root, polite | 400 ms | Yes | |
| Setting saved | "Saved" | Root, polite | 800 ms, burst coalesced | Yes | |
| View opened (route change) | "Settings" window title and focus on h1 | none (focus announces it) | n/a | n/a | No status message, focus does the work. |
| List filtered (sidebar, gallery) | "12 results" | Root, polite | 500 ms | Yes | Skips if count unchanged. |
| Palette results | "6 results" | Palette local status | 300 ms | Yes | |
| Item deleted, reverted | "Chat deleted", "Changes to src/main.rs reverted" | Root, polite | Immediate | Yes | Always with an Undo where possible, reachable from the visible banner. |
| Queued message | "Message queued" | Root, polite | 400 ms | Yes | |
| Loading (skeleton) | "Loading projects" | Root, polite | Shown after 400 ms delay, not for fast loads | Yes | |
| Streaming tokens, reasoning tokens, terminal output lines | none | none | n/a | Never | Hard rule: nothing at token or line granularity. |

### 7.3 Final answer insertion

For screen reader users, and for everyone as the final structure, the assistant message body is committed once on completion. Implementation contract:

- While streaming, the visible body is rendered in a streaming surface (`aria-hidden="true"`), word by word.
- On completion the structured, final markup (paragraphs, lists, code blocks with copy buttons, headings demoted to level 5 inside messages) replaces the streaming surface in a single DOM update, and the article loses `aria-busy`.
- If the user has "Screen reader optimised layout" on, the streaming surface is visually collapsed to a single line "Barry is writing" with elapsed time, and the final body appears on completion.
- Focus is never moved to the answer. Alt+Up from the composer focuses it. Screen reader users who prefer automatic reading can enable the read-aloud option from 7.2.
- A stopped or failed generation commits whatever text exists the same way and adds the state chip.

## 8. Component accessibility behaviour not covered elsewhere

### 8.1 Streaming and reasoning surfaces

- The streaming and reasoning visual surfaces are `aria-hidden="true"` and read-only. They are never focusable.
- Screen reader virtual cursors that sit inside the transcript when completion occurs may lose position because the node is replaced. The final markup keeps the same `<article>` element and its stable `id`; only the body inside is replaced. Test case SR-05 checks whether this preserves the reading position in NVDA browse mode, JAWS virtual cursor and Narrator scan mode.
- Sighted keyboard users can pause auto-scroll: any scroll gesture, Page Up, or Tab into the transcript stops following.

### 8.2 Dialog behaviour

- Native `<dialog>` and `showModal()` supply inertness, focus containment and Escape handling. The app adds: `autofocus` target, `aria-labelledby`, `aria-describedby`, local status, and focus restoration to the invoker (stored before opening, falling back as in the focus rules).
- `cancel` event handling maps Escape to the safe outcome for permission and confirmation dialogs.
- `close` event returns focus. The element is closed through `dialog.close(returnValue)` so callers get the decision.
- The scrim is `::backdrop` with `--color-scrim`. In forced colours the dialog has a visible 2 px border.
- Dialogs never open on top of each other: a second request queues behind the first.

### 8.3 Forms and errors

- Every control has a visible, programmatically associated label. Placeholders are hints only.
- On submit with errors: an error summary is not required for single-field inputs, because errors are inline and tied via `aria-describedby`, and focus goes to the first invalid control. For the multi-field provider dialog, a summary heading "2 problems" (`tabindex="-1"`) with links to fields receives focus.
- Error text starts with "Error:" in visually hidden text for screen readers if the visible text starts with an icon only.
- Required fields use the `required` attribute and visible "(required)" text, not asterisks alone.
- Autocomplete tokens are set where applicable (`autocomplete="off"` for API keys, with explanatory hint).

### 8.4 Text alternatives

- Generated images: alt is the generation prompt condensed to a sentence by default, editable by the user. Never "image".
- Decorative: bear mark, icons next to text, dividers, empty states art: `aria-hidden="true"` or empty alt.
- Video: always native controls. Captions when the source provides them. A visible description field is read via `aria-describedby`.
- Charts, if added for cost and usage: accompanied by a data table. Not specified in this version.

## 9. Responsive, zoom and display adaptation

| Condition | Required behaviour |
|---|---|
| 200 percent zoom (640 CSS px wide window equivalent at 1280) | No loss of content or function. Sidebar is a drawer, inspector a drawer, composer sticky. |
| 400 percent zoom (320 CSS px wide) | One column. No two-dimensional scrolling except inside code, diff and table wrappers (permitted by WCAG 1.4.10). Header buttons wrap, labels visible for the main four (sidebar, palette, settings, more). |
| Text spacing override (WCAG 1.4.12) | No clipped or overlapping text. All text containers have `min-block-size`, not fixed heights. |
| 150 percent app text size | Layout must hold, checked manually. Same rules as above. |
| Windows text scaling up to 225 percent | WebView2 respects the setting. Layout checked manually alongside zoom. |
| `forced-colors: active` (all four Windows contrast themes) | All content visible, borders on all controls, focus outline in `Highlight`, no information lost without colour. |
| `prefers-contrast: more` | Stronger border, text-subtle raised to muted level. |
| `prefers-color-scheme` | Automatic unless the user selected a theme. Theme switch applies without reload. |
| `prefers-reduced-motion: reduce` | Durations near zero, spinner static, no caret blink, no smooth scroll. |
| Touch | 44 px targets, no hover-only functions, no path-dependent gestures. |
| Pointer cancellation | Actions fire on `click`/pointer-up, never on pointer-down, and dragging is never required (sidebar resize also has Ctrl+Left and Ctrl+Right when its handle has focus, 16 px steps). |
| RTL | Logical properties. Directional icons mirror. Code stays LTR (`dir="ltr"` on code and diff). |
| Window resize | Layout reflows live. No fixed pixel widths. |

## 10. Manual screen reader test matrix and checklist

All statements in this specification about screen reader behaviour are unverified until this matrix is executed. Run before every release, record results in the release notes, and file every failure as a defect that blocks release unless explicitly accepted with a mitigation.

### 10.1 Environments

| ID | Screen reader | Host | Notes |
|---|---|---|---|
| E1 | NVDA, latest stable | Barry packaged build (WebView2 Evergreen) | Primary test. Browse mode and focus mode. Speech viewer enabled to log output. |
| E2 | JAWS, latest stable | Barry packaged build | Virtual cursor and forms mode. Check settings centre verbosity defaults. |
| E3 | Narrator | Barry packaged build | Scan mode on and off. Use Narrator with Edge WebView2 UIA exposure. |
| E4 | NVDA | Microsoft Edge on the `trunk serve` dev page | Isolates app issues from Tauri shell issues. |
| E5 | Voice Access (Windows 11) | Barry packaged build | Label in Name check ("click Send message"). |
| E6 | Windows Magnifier, 200 percent and 400 percent zoom, no screen reader | Barry packaged build | Visual reflow and focus visibility. |
| E7 | Windows contrast themes (Aquatic, Desert, Dusk, Night sky) | Barry packaged build | Forced colours. |
| E8 | Keyboard only, no screen reader | Barry packaged build | Full workflow without a pointer. |

Test hardware and speech settings: one machine with default speech rate and a second tester who is a daily screen reader user, if available. Record screen reader version, Windows build, WebView2 runtime version, and Barry build.

### 10.2 Test cases

Each case lists the steps and the expected result. Pass criteria apply to E1, E2, E3 separately.

| ID | Scenario | Steps | Expected |
|---|---|---|---|
| SR-01 | Launch and orientation | Start the app with a project open. | Window title read. Focus in composer, name "Message to Barry, edit, multi-line". Landmarks listed by the screen reader's landmark list: banner, navigation Projects, main, complementary Inspector, contentinfo. |
| SR-02 | Landmark and heading navigation | Use landmark and heading navigation commands in browse mode. | Sequence matches section 4. No unlabelled duplicates. Heading levels do not skip. |
| SR-03 | Project tree | Tab into the tree, use arrows, expand, collapse, open a chat, rename. | Role, name, level, expanded state and position spoken. Arrow keys work in focus mode and in browse mode. If any of the three fails to expose tree semantics correctly, switch to the flat list fallback (decision D-2). |
| SR-04 | Send and generate | Type a message, press Enter. | Focus stays in the composer. Exactly "Generating response" is announced once. No token-level speech. Stop button reachable by Tab, named "Stop generating". |
| SR-05 | Completion | Wait for the response to finish. | "Response complete, N words" is announced once. The answer is present in the transcript. Alt+Up focuses it, and it is read as an article with heading "Barry, time". Browse cursor position is not reset to the top of the page. |
| SR-06 | Streaming silence | Generate a long response with the browse cursor parked in the transcript. | Nothing is read during streaming. No repeated announcements. No reading position jumps. |
| SR-07 | Stop | Press Ctrl+. and Stop button during generation (two runs). | "Stopped. Partial response kept". Focus on the composer. Partial text present as a complete message with a "Stopped" chip. |
| SR-08 | Reasoning disclosure | Move to a completed message, expand and collapse Reasoning. | Name "Reasoning", state expanded or collapsed, content readable by arrow keys. During streaming the reasoning is not read. |
| SR-09 | Tool-call cards | Navigate a message with three tool calls by list and heading commands. | List "Tool calls, 3 items". Each card heading names tool and target. Status in text. Details disclosure works. |
| SR-10 | Permission dialog | Trigger a command permission request while the composer has focus. | Dialog announced with its title and description. Focus on Deny, announced "Deny, button". Tab order: command, Deny, Allow once, Allow for this chat. Enter denies. Escape denies. Closing returns focus to the composer. Result announced once. |
| SR-11 | High-impact permission | Trigger a delete-outside-project request. | Title begins "High impact". Checkbox gating Allow once is announced, disabled button explanation read. "Allow for this chat" absent. |
| SR-12 | Permission while not focused | Switch to another application while Barry requests permission, then return. | Window title or taskbar flash indicates attention. Returning to the app, dialog is announced and focus is on Deny. |
| SR-13 | Diff table | Open a file edit, navigate the table by cells, rows, F8. | Caption read. Column headers read with cells. Changed rows read "Added:" or "Removed:" before the code. Colour is not needed. Hunk headers navigable. Summary list mode works. |
| SR-14 | Terminal pane | Run a command that takes 10 seconds. | While running, only the "Running" status is exposed, no output chatter. On finish "Command finished, exit code N, M lines of output" once. Output readable as a block with arrow keys and the region is scrollable by keyboard. |
| SR-15 | Command palette | Open with Ctrl+K, type, arrow, run. | Dialog name read. Combobox role, expanded state, active option read as arrow keys move. Result count announced once per typing pause. Escape returns focus. |
| SR-16 | Settings | Walk every section with Tab, change a theme, toggle a switch, change a shortcut. | Labels, groups (fieldset and legend), switch states and hints read. "Saved" announced once per change burst. Shortcut capture dialog states how to cancel. |
| SR-17 | Media gallery | Open the gallery, filter, open an image and a video. | List "Media items, N items". Item names include type and size. Alt text read. Lightbox dialog announces its title. Video controls operable by keyboard. No autoplay. |
| SR-18 | Errors | Disconnect the network, send a message. | Error announced once with recovery text. Retry button reachable. After reconnecting, "Back online". Decision D-1 evaluated here. |
| SR-19 | Route changes | Switch chats from the tree, open Settings and back. | Window title changes. Focus on the h1, which is read. No duplicate announcement from the status region. |
| SR-20 | Skeleton and loading | Throttle disk, open a large project. | "Loading projects" announced only if loading exceeds 400 ms. Content appears without focus loss. |
| SR-21 | Icon-only buttons | Tab through the header, composer and tool cards. | Every icon-only button has an action name. No "graphic", "image" or file names are read. |
| SR-22 | Delete flow | Delete a chat from the tree. | Confirmation dialog focuses the least destructive button. After confirmation, focus to the next row and "Chat deleted" announced. |
| SR-23 | Queue and multi permission | Trigger three permission requests in succession. | Title count "1 of 3" read. Focus on Deny each time. No dialog stacking. |
| SR-24 | Read-aloud option | Enable "Read full response when complete" and generate a short response. | "Response complete" followed by the response text exactly once. Disabled for responses over the limit with the Alt+Up hint. |
| SR-25 | Modal local announcer | While the palette is open, trigger a background completion. | Nothing is spoken until the palette closes, then the final state once. Palette result counts are spoken from its local region. |

### 10.3 Keyboard, zoom and display cases

| ID | Scenario | Expected |
|---|---|---|
| KB-01 | Every shortcut in section 6 under E1, E2, E3 in focus and browse (scan) modes | The app receives it, the screen reader does not swallow it. Record any exception and rebind. |
| KB-02 | Full workflow with keyboard only (open project, send, stop, grant permission, review diff, change a setting, open gallery) | Completed without a pointer. No focus loss to body. No keyboard trap. |
| KB-03 | Visible focus on every control in all themes | Ring always visible, not obscured by sticky elements, at least 3:1. |
| KB-04 | Skip links | Visible on focus. Move focus to the right target. |
| KB-05 | Tab order matches visual order in wide and narrow layouts | Yes. |
| ZM-01 | 200 percent zoom | All content and function available, no horizontal scroll apart from code, diff and tables. |
| ZM-02 | 400 percent zoom (320 CSS px) | One column, reflow, composer reachable, dialogs fit and scroll. |
| ZM-03 | Text spacing bookmarklet (WCAG 1.4.12) | No clipping or overlap. |
| FC-01 | Windows contrast themes (four) | All controls have visible boundaries, selected states visible, icons visible, diff readable without colour, focus visible. |
| FC-02 | Light, dark, contrast and system themes | Contrast ratios as in `design-system.md`. No unreadable text. |
| RM-01 | Reduced motion on | No animation beyond opacity step. Spinner static. No auto-scroll animation. |
| VA-01 | Voice Access | "Click Send message", "Click Stop generating", "Click Deny" work. Visible labels match names. |
| TS-01 | Target size | All targets at least 24 by 24 CSS px, primary targets at least 44. Check with the overlay of DevTools. |
| TP-01 | Pointer only | Everything works with touch and mouse. No hover-only features. |

### 10.4 Release checklist

Mark each as pass, fail or not applicable per environment before release.

- [ ] E1 NVDA: SR-01 to SR-25, KB-01
- [ ] E2 JAWS: SR-01 to SR-25, KB-01
- [ ] E3 Narrator: SR-01 to SR-25, KB-01
- [ ] E4 NVDA on the Edge dev page: SR-04, SR-05, SR-06, SR-10, SR-13, SR-15 (to isolate shell issues)
- [ ] E5 Voice Access: VA-01
- [ ] E6 Magnification: ZM-01, ZM-02, ZM-03, KB-03
- [ ] E7 Contrast themes: FC-01, FC-02
- [ ] E8 Keyboard only: KB-02, KB-04, KB-05, TS-01
- [ ] RM-01 and TP-01
- [ ] Automated checks (axe-core on all views and dialogs) show zero violations
- [ ] Decisions D-1 (error announcement strategy), D-2 (tree or flat list) and D-3 (side-by-side diff exposure) recorded with the evidence from the matrix
- [ ] Open defects reviewed and each either fixed or accepted in writing with a workaround

### 10.5 Decisions pending test evidence

| ID | Question | Default | Switch if |
|---|---|---|---|
| D-1 | Are polite errors reliably spoken? | Polite status for errors | A screen reader drops or delays errors behind other speech: add one separate `role="alert"` region, used for errors only |
| D-2 | Project tree or flat list? | Tree pattern | Any of the three screen readers mis-announces levels, expanded state or position, or arrow keys do not work in browse mode |
| D-3 | Should side-by-side diff be exposed at all? | Visual only, unified table exposed | Testers find a two-column table usable: expose it |
| D-4 | Does replacing the answer body on completion disturb the browse cursor? | Replace the body inside the same article | SR-05 shows the cursor reset: append the final markup after the streaming surface and hide the surface instead of replacing |
| D-5 | Alt+Up and Alt+Down for message navigation | Keep | KB-01 shows a conflict in any of the three screen readers: use Ctrl+Up and Ctrl+Down |
