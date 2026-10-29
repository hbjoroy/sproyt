# Sprøyt layout and visual design

Preserve the editorial character: warm paper or charcoal surfaces, strong destination names, restrained rules and familiar outline icons. Improve alignment and density before adding cards, navigation layers or decorative effects.

## Source and tokens

`packages/sproyt-ui` is the maintained component source. Keep its design and component contracts aligned with the skill references. Ship package changes through a versioned frontend tarball; do not edit node_modules. Reuse `--sp-*` theme tokens in light and dark themes.

Body and composer text stay at 16px. Names and channel titles use 14–16px in compact layouts; contextual labels and status use 12–13px. Small metadata must remain readable and support zoom. Do not shrink primary content to make controls fit.

## Header and identity

Reserve space for circle and channel identity before secondary actions. Compact headers use a readable circle overline and channel title. Never abbreviate a circle to one letter as its only visible identity, and never combine manual character slicing with CSS ellipsis. Allow deliberate wrapping where useful; unusually long names remain available through an accessible title disclosure.

Keep global actions and channel actions grouped, not distributed across the available width. Compact secondary channel actions may live in the existing title disclosure. Preserve inbox state, connection recovery, settings, history and thread discoverability; do not add a second navigation architecture. Menus share an icon gutter and text baseline even when an item has no icon.

## Responsive shell

Use container width and actual available space, not device identity. The app list/detail threshold is 900px; above it, navigation is at least 240px and the main region receives the remaining width. A thread splits alongside its channel only when the content region has at least 780px. These thresholds serve separate purposes. Preserve reading position, drafts and focus across transitions.

Use common left edges for channel identity, timeline and composer. Keep prose readable within a bounded column (up to 860px), while rich media may use available width within its own constraints. Do not stretch short messages just to fill a large monitor.

## Message structure and rhythm

Use an explicit identity area and a single non-wrapping action group. Name, badge, time, edit state and profile status must not compete as unrelated flex children. Secondary metadata may occupy a deliberate second line; actions must never wrap one by one or create a separate empty toolbar row above the body.

Keep the early-adopter badge visible. Informational inline metadata is not a full-size toolbar button: provide accessible details without assigning every decoration a separate 44px column. Keep real reaction/reply/menu actions reachable with keyboard and touch. Do not make discovering threads depend solely on hover or long-press.

Message body follows the identity area closely. Use 8px between successive messages. Enclose only the full byline/action row in a quiet 1px frame, including when identity wraps. Use the normal canvas inside the frame (`--sp-message-header`), a 25% line/canvas mix (`--sp-message-divider`), 2px corners and a static 6px outer glow with 1px spread (`--sp-message-glow`: accent at 12% opacity in light mode, 6% in dark mode). Keep the glow barely perceptible; never animate it or add a second colored surface. Inset header text by 6px and the action group by 2px without increasing row height. Dark mode softens header names to a 90% text/canvas mix and secondary header information to a 95% muted/canvas mix, with the same treatment for an automatic dark system theme. Body/composer text retains its normal contrast. The frame groups the sender with their message below; keep body/media on the normal canvas without a surrounding message card. Preserve compact row height, reachable controls and distinct date/unread boundaries. Masthead/sidebar continue to share `--sp-chrome`; do not add another green surface hue.

Rendered Markdown uses normal block whitespace and controlled paragraph/list margins; preserve whitespace only inside code or intentional plain text. Avoid doubled gaps from source newlines plus block margins. Bound tables, links and media to the content width.

## Input and targets

Keep the idle composer to one row, full remaining text width, with attachment context visible when present. Grow to about four lines before internal scrolling. Preserve controlled drafts, attachment-only sending, IME, sending shortcuts and accepted-send clearing. Focus must be visible without a heavy extra frame that clips against the composer.

Aim for 44px actual primary touch targets and 32–36px fine-pointer actions. Visual glyph size and target size are different. Inline information may use appropriate spacing or a shared accessible details action rather than inflating the whole byline. Never overlap hit areas.

## Validation fixtures

Check 320, 390, 430, 820, 900/901 and 1440 CSS pixels, light/dark, coarse pointer and keyboard, plus enlarged text. Use long circle/channel/person names, status, early-adopter badge, edited time, reactions, threads, tables and mixed-orientation media. Verify no page-level horizontal overflow, no separated action rows, visible channel identity, menu dismissal/focus restoration and intact composer/scroll behavior. Production observations are read-only; use isolated fixtures for sending and destructive actions.
