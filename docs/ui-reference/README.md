# Sprøyt UI visual reference

These screenshots are the review evidence for the design-system restoration.
The `original-*` images render the React example from the design-system source;
the matching `app-*` images render Sprøyt with the same five-message fixture.

The checked widths cover the narrow supported boundary (320 px), an ordinary
mobile viewport (390 px), and desktop (1440 px). The 390 px pair also records
the dark theme. The browser contract in
`frontend/tests/ui-react-visual-restoration.spec.ts` regenerates the app-side
light/dark evidence and checks the canvas colour, message rule, metadata reset,
quiet accepted state, growing draft and usable send control.
