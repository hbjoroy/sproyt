# Conversation presentation adapter

This is an opt-in presentation boundary, not an enabled replacement UI. It uses
the installed `@sproyt/ui/react` components and receives the existing application
runtime. It never constructs a connection, session controller, outbox or mailbox.

For local development, append `ui=react` to a localhost, 127.0.0.1 or IPv6
loopback URL. The isolated preview shows live navigation and escaped message
source from the existing runtime. Its return button restores the full UI in
place, preserving the connection and drafts. The preview's controlled text
Composer uses the same draft, admission, durable outbox and receipt/failure paths
as the full UI. Enter/Shift+Enter and IME behavior follow the existing input policy.
Reply buttons now open the real thread through the host's existing load command.
Root and thread composers keep separate drafts and share the existing outbox;
load failures can be retried and rejected replies restore their draft. The
design-system ThreadPane splits beside the channel when wide and replaces it
when narrow without unmounting the channel. Closing returns focus to the root's
reply button. The hidden legacy thread remains inert and nonmodal until a
handoff promotes it to the full modal composer.
Mentions and image generation hand back to the full composer without sending or
clearing the draft. Markdown, Mermaid diagrams, message media and invitation
cards use the established safe renderer inside an isolated React-owned node;
their command and response handling remains host-owned. Attached media uploads
through the existing host transport. Administration and other incomplete parity
flows remain in that full UI. Other
hostnames and URLs without this explicit query retain the default UI.

## Integration seam

1. Keep the one `createApplicationRuntime(renderServerEvent)` in `app.ts`. Pass it
   as `runtime`; connection updates subscribe through `useSyncExternalStore`.
2. `getConversationSnapshot()` in `app.ts` now projects the existing domain state
   through `application/conversation-snapshot.ts`. It supplies detached, immutable
   navigation groups, selection, root messages, notices, thread metadata and
   connection state. Its groups and messages match this adapter's props. Call it
   after host state changes and pass updates to the view; it creates a new object
   on every call and is not a `useSyncExternalStore` getter.
   `createConversationViewProps(snapshot, bindings)` now maps this projection to
   the view, including root/thread messages, system notices, group actions and
   independent channel/thread composer targets. Per-channel notification
   commands are connected through the host binding described below; remaining
   command adapters and host update notifications stay explicit.
3. Call `mountConversationView(emptyContainer, props)` once in the development
   UI selector. Install the existing shared stylesheet once, and give the
   container a definite height. Do not run the legacy renderer in that container.
4. Call the returned `update(props)` after application state changes. Supply
   host-filtered navigation groups with stable IDs, labels, unread counts, group
   actions and selection/search callbacks. Group IDs must not be display names.
5. Supply timeline messages, the active channel ID, existing timestamp formatting,
   safe content rendering, permitted actions, pagination and scroll/read policy.
   `LegacyContent` can host the current safe Markdown/media renderer in an isolated
   node; return cleanup that cancels asynchronous rendering and listeners. Keep
   its render callback stable until the rendered content actually changes.
6. Pass the adapted composer as `composer`, the design-system `ThreadPane` as
   `thread`, and focused dialogs as `overlays`. These slots deliberately do not
   replace attachment-only sending, drafts, invitations or reaction policy with
   the library demo. The channel remains mounted when a thread opens.
7. Save/restore channel scroll positions and focus in the host when switching
   conversations. Timeline `viewportRef`, `onScroll`, stable message IDs and the
   load-older callback provide the hooks; this adapter does not force-scroll.
8. `unmount()` releases the React tree and subscriptions, never the runtime.

The concrete host boundary is `host-adapter.tsx`. Its required `renderComposer`
receives `{ channelId, parentMessageId }` and must return the host's composer with
its existing attachment-only send and draft behavior. Its `message` binding
supplies content, actions, reaction requests, status and timestamp rendering for
both timelines and the thread parent. `renderGroupActions` receives the original
group, while `renderConversationAction` receives each original conversation
including its notification state. Notification subscriptions do not imply a
muted conversation. `navigationActions`, `contextActions` and `overlays` retain
inbox, membership, invitation and media/dialog features. `timeline` and
`threadTimeline` supply host pagination, errors, retries and viewport hooks.
The adapter forwards the thread viewport ref and scroll callback to the design
package's actual `.sp-thread-replies` scroll container, including the parent
message. The root timeline itself is the root scroll container.
`ConversationHost` is also available for a parent React tree; it does not mount
anything by itself.

The adjacent `conversation-view.test.ts` is a Node test entry that can be bundled
with esbuild (`platform: node`, `format: cjs`, `.cjs` output) and run with `node --test`. It covers
root/thread isolation, escaped text, deleted body suppression, duplicate group
names, and the read-only runtime boundary. `host-adapter.test.ts` additionally
covers scoped composers, slot/callback forwarding, notices, missing/deleted
thread parents, missing selection, and context while filtering navigation.
These tests are not yet wired into the existing
boundary-test entry point because this increment changes only this directory.

Before enabling in production, complete composer/reactions/media adaptations,
feature parity, client rendering and keyboard tests, scroll/focus checks, and the
repository's full migration/release gates. Server rendering tests do not validate
browser layout or interaction.

The local development preview exposes `PreviewManagement`, a design-system
`Dialog` with a searchable circle directory and explicit full-interface handoffs.
It opens the existing complete creation, membership, invitation/enrollment,
people/direct-message, channel/Grafana, profile, notification, inbox, agent and
Heart flows. The host retains permission checks and feature visibility. Opening
the menu performs no mutation; leaving the preview retains channel/thread drafts,
uses the same connection and selects the requested dialog or sidebar section.
This is temporary parity access, not a completed React migration of those forms.
`ui-react-management.spec.ts` covers dismissal/focus, scoped handoffs, compact
sidebar access and channel/thread draft retention.
