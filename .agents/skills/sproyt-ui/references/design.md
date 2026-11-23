# Sprøyt layout and visual design

## Kretsrom

Place the personal status emoji immediately after the sender name on desktop and mobile. Use a speech bubble for text-only status. Press or use the keyboard to read the full status, with editing available to its owner. Show status text in the remaining byline space; let it shrink before the sender and time. Use a 20px visible reaction pill inside the 26px action row (32px on touch, at least 44px wide). Keep media captions and original/download controls together in a 24px row (32px on touch). Keep 6px between messages.

Use the selected Pratebok and Kretsen først prototypes for this edition. Kretsrom owns its layout through KretsromShell and KretsromMessage. The editorial layout rules below apply to the earlier edition.

The visual baseline is the Hyttegutta / #Prat scene in the archived kretsrom.html. Use verkstad-aktiv.html for attachment tools and workshop states. The package ships the scoped stylesheet as @sproyt/ui/kretsrom-reference.css. In the Sprøyt repository, build the comparison with npm --prefix packages/sproyt-ui run build:kretsrom. Open the two HTML files in packages/sproyt-ui/demo-kretsrom directly.

- Lead the directory with Vennekretsar, a small initial tile, the circle name and a disclosure. Indent channels on a fine rule. Show the latest visible sender and excerpt beneath a channel when available. Give the selected row a paper surface, soft corners and a narrow green leading edge. Keep settings and notification preferences reachable through secondary actions.
- Leave sender, time and message on one continuous surface. Use muted actions under the words, a fine unread rule and a modest composer. Compare the same scene at 740px and on a phone, including light and dark. Match text hierarchy, spacing and surfaces before adjusting breakpoints.
- At integration, compare both editions using real host flows. Exercise sidebar and thread resizing by drag and keyboard, reload and reset. Switch circles and DMs, collapse a circle and reveal a hidden channel. Check the wave shortcut, drafts through navigation and send failure, saved reactions, replies, attachment-only sending, and closing and reopening a working or completed workshop. Verify access to search, notifications, settings and channel tasks. Prototype fixtures establish appearance and interaction intent. Record host parity separately before rollout.

- A 44px masthead carries the wave and serif Sprøyt wordmark. The room header below it identifies the circle and channel together. On a phone, show the back-to-circles action on the left and the destination on the right. Keep the wave shortcut.
- The warm directory groups channels beneath collapsible circle headings. Keep hidden channels in an expandable subgroup. On narrow screens directory and conversation occupy the same viewport in turn.
- Use a 32px sender disc, 10px text gutter, a plain byline and a continuous paper reading surface. Keep date boundaries in the feed. Reactions and replies sit below the message.
- Compare a message with two reactions and replies. The order on a single wrapping line is a rounded emoji/count button, Reager, the reply count (or Svar) and secondary actions. Use a tinted fill for the viewer's vote. Keep participant details and keyboard focus. Open the picker above the composer as a compact rounded paper panel. Offer one deduplicated set of choices, with saved positions first. Put search, pin management and custom Unicode under Fleire.
- The composer is a single rounded strip with a leading tool button and trailing send button. Keep in-progress media and thread context near the draft. Place the private workshop beside the conversation on wide screens and at the bottom on phones. Reopen an active job from its conversation.
- Reuse the host's membership, draft, selection, upload, reaction and workshop owners. Additional destinations keep their existing behaviour when imported.



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
