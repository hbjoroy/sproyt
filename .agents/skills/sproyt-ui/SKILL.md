---
name: sproyt-ui
description: Build React or Vue interfaces with the Sprøyt editorial design system, using its bundled components, light and dark themes, and compact layouts. Use when Sprøyt is requested or an existing project already uses @sproyt/ui; do not impose this style on unrelated projects.
---

# Sprøyt UI

Use the bundled `@sproyt/ui` package for a consistent editorial application interface. Preserve the host project's framework, routing, data ownership, and package manager. This skill provides components and design guidance; it does not authorize publishing, transmitting messages, or changing production data.

## Integrate

1. Inspect the target project's package manifest and existing UI entry point. If `@sproyt/ui` is present, use the installed version and do not overwrite it blindly.
2. Otherwise install `assets/sproyt-ui-0.1.0.tgz` from this skill's directory with the project's package manager. Resolve the path relative to this skill, not a remembered workspace. Prefer copying the archive into the target project's `vendor/` directory so local dependency references survive moving the project. The archive is a portable, built package, not an npm registry publication.
3. Import `@sproyt/ui/styles.css` once and import components from `@sproyt/ui/react` or `@sproyt/ui/vue`. Use React 18/19 or Vue 3.5+ respectively. A consuming project needs only its chosen framework.
4. Wrap the relevant application region in `Theme`. Choose `mode="system"`, `"light"`, or `"dark"`; use citron by default. Set an explicit height when using the scrollable `AppShell` in a full app or widget.
5. Read [component contracts](references/components.md) for APIs and examples; read [design rules](references/design.md) when composing new screens or extending the library.

## Preserve the interaction contracts

- Components are controlled: the host owns messages, drafts, replies, selected conversations, loading and failures. `Composer` emits a trimmed nonempty string but does not clear its value. Clear drafts only when sending is accepted; preserve them on failure.
- Vue uses `v-model` for `TextField`, `Composer`, and `EventCard`; React uses `value` and `onChange`. `ConversationList` uses Vue `@select` / React `onSelect`.
- `Dialog` uses a native modal dialog; keep it inside `Theme`, control `open`, and close it on its emitted/callback close event. Native Escape, focus containment, and restoration are retained.
- Breakpoints respond to container width. Compact navigation is explicit list/detail state; keep drafts and reading position when navigating. Touch density remains generous even in a narrow desktop widget.
- Keep visible action labels, human-readable identity, local error recovery, and meaningful empty states. Use locale-specific text through props/slots; Norwegian defaults are examples, not a localization system.

## Validate the integration

Build/typecheck with the target project's existing commands. Try keyboard-only sending and modal dismissal, a compact container, long names, both themes, and failure recovery for any connected mutations. These are starter web components, not native controls; use the semantic tokens and behavior contracts as references for later native implementations.
