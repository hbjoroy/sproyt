# Design rules

The accepted direction combines editorial hierarchy with social warmth: purposeful rules, strong destination headings, readable sans-serif UI text, sparse monospaced metadata, and a serif for authored invitations. Avoid turning every message into a card.

## Tokens

All CSS is scoped under `.sp-theme`. Override `--sp-*` semantic variables on a Theme instance; do not add a second independent palette. The exported `themes` object from `@sproyt/ui/tokens` mirrors the core light/dark colors for other renderers.

Light: warm paper `#faf9f5`, dark ink `#191b18`, white surfaces. Dark: charcoal `#191b18`, darker chrome `#141612`, raised surfaces `#22261f`, ivory `#ebece1`. Citron `#d9ed70` and periwinkle `#b9c5ed` are alternative brand accents; error is separate. Dark selection uses an accent edge and quiet fill to avoid large luminous blocks.

Body 16px, labels 14px, metadata 12–13px, headings 24–32px. Spacing 4/8/12/16/24/32. Mostly square surfaces, 2px control corners, 1px dividers, 3–4px structural rules. No decorative shadows, gradients or glass required. Accent never substitutes for text or shape when conveying status.

## Composition

`AppShell` provides masthead, scrollable navigation and a flexible main region. In main, compose `.sp-context`, `.sp-timeline`, and `Composer`. At container widths <=650px the list and detail are separate views controlled through `view`. `.sp-only-compact` marks a Back button. The shell needs a containing height, e.g. `height:100dvh` or a bounded widget height.

Person directories, tasks, settings and ordinary forms should use the same field and row treatments. Keep group context above channel names. Unread counts, selection, and muted state have different meanings. Technical identifiers stay in details.

For media, use bounded uncropped previews and an explicit full-size action. Preserve reading position when new messages arrive; do not force-scroll readers away from older content. Keep routine contextual details inline or in a dedicated view; use dialogs for a focused temporary task.

## Inputs and adaptations

Comfortable targets are 44px minimum; compact density is 36px for fine pointers and remains 44px for coarse pointers. Container size and input type are separate axes. Maintain keyboard focus visibility, native controls, descriptive labels, and reduced-motion behavior. AppShell does not manage router history, authentication, persistence, list virtualization, or network state.

## Message density

Keep message body text at 16px. Place reply/reaction controls alongside author and timestamp, keeping them always reachable. Use a 12px inter-message gap and compact internal spacing rather than adding message cards. Pointer message actions are 32px high; coarse-pointer actions stay at least 44px. The composer starts at one line, keeps its accessible label and keyboard hint, and remains manually resizable. Routine empty status rows do not consume space. Put application-specific attachment tools in a small secondary composer row.

## Responsive chat and composition
Use container width rather than device identity. At 800px or less, use list/detail navigation; split a thread alongside the channel only when the content region has at least 780px. Preserve independent drafts and reading position. Keep the idle composer one row, reveal optional tools during writing, and leave selected attachment context visible. Grow the text field to 3½ lines before scrolling inside it.

Message rhythm: keep bylines close to their text, use an 8px gap between messages, and place a faint 56px rule 4px above each subsequent byline. The short rule introduces the next sender. Keep body type readable and retain 44px touch targets.
