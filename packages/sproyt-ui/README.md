# Sprøyt UI — 0.2.3

Sprøyt sin nye klientutgåve er dokumentert i [Kretsrom](../../docs/kretsrom.md), med oversikt over vurderte og importerte flater.

Reusable editorial web components for React 18/19 and Vue 3.5+, with matching light/dark themes and compact container layouts. The demo is a local simulation, not a messaging service.

## Use in another project

Copy `../../frontend/vendor/sproyt-ui-0.2.3.tgz` into that project's `vendor/` folder and run `npm install ./vendor/sproyt-ui-0.2.3.tgz` (or your package manager's equivalent). Install only your chosen framework. Import `@sproyt/ui/styles.css` once and components from `@sproyt/ui/react` or `@sproyt/ui/vue`.

See [component contracts](docs/components.md) and [design rules](docs/design.md).

## Kretsrom reference

Run `npm run build:kretsrom` in this package and open `demo-kretsrom/kretsrom.html` or `demo-kretsrom/verkstad-aktiv.html`. Run `node scripts/check-kretsrom-reference.mjs` to compare the rendered scene and exercise the controls with Chromium. Both builds use the Sprøyt frontend development dependencies.

## Develop and try

```sh
npm ci
npm run build
npm run dev
```

Open the displayed localhost URL. The React and Vue versions use their respective library components. Theme and size controls let you try both appearances and a compact widget. Messages, drafts, reactions and RSVP choices are simulated locally in your browser. No messages are sent to other people.

```sh
npm test
npm run check:demo
npm run build:demo
npm run pack:frontend
```

The library builds to `dist/`; the demo builds to `demo-dist/`. The archive is not published to a registry. Production applications must supply routing, authentication, persistence, network error handling and any media-upload service. This release covers the initial chat and generic-form components, not every component in the earlier design-system inventory.
