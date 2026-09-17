/** Opt-in, on-device geometry only: no draft/message contents or network calls. */
export function installViewportDiagnostics(): void {
  if (!new URLSearchParams(location.search).has("viewport-debug")) return;
  const panel = document.createElement("pre");
  panel.id = "sproyt-viewport-diagnostics";
  panel.setAttribute("aria-label", "Viewport diagnostics");
  Object.assign(panel.style, {
    position: "fixed", left: "4px", right: "4px", zIndex: "2147483647",
    margin: "0", padding: "6px", border: "1px solid #888", background: "#111e",
    color: "#fff", font: "11px/1.3 monospace", whiteSpace: "pre-wrap",
    pointerEvents: "none", maxHeight: "42%", overflow: "hidden"
  });
  document.body.append(panel);
  const n = (value: number) => value.toFixed(1);
  let before = "";
  let focused = "";
  const update = () => {
    const viewport = window.visualViewport;
    const top = viewport?.offsetTop ?? 0;
    const bottom = top + (viewport?.height ?? innerHeight);
    const root = document.querySelector("#sproyt-react-preview");
    const input = Array.from(document.querySelectorAll<HTMLTextAreaElement>("#sproyt-react-preview textarea"))
      .find(element => element.getClientRects().length > 0);
    const form = input?.closest("form");
    const box = (element: Element | null | undefined) => {
      if (!element) return "—";
      const rect = element.getBoundingClientRect();
      return `${n(rect.top)}..${n(rect.bottom)} h=${n(rect.height)}`;
    };
    const css = input && getComputedStyle(input);
    const formCss = form && getComputedStyle(form);
    const inputBottom = input?.getBoundingClientRect().bottom;
    const outline = css && css.outlineStyle !== "none"
      ? parseFloat(css.outlineWidth) + parseFloat(css.outlineOffset) : 0;
    const isFocused = document.activeElement?.tagName === "TEXTAREA";
    const summary = `vv=${n(viewport?.height ?? innerHeight)} top=${n(top)} app=${root ? n(root.getBoundingClientRect().bottom) : "—"}`;
    if (isFocused) focused = summary;
    else before = summary;
    panel.style.top = `${top + 64}px`;
    panel.textContent = [
      "Viewport diagnostic v1 (CSS px)",
      `inner=${n(innerHeight)} client=${document.documentElement.clientHeight} scrollY=${n(scrollY)}`,
      `vv=${n(viewport?.height ?? innerHeight)} top=${n(top)} bottom=${n(bottom)} scale=${n(viewport?.scale ?? 1)}`,
      `app viewport=${document.documentElement.dataset.appViewport ?? "—"}`,
      `app ${box(root)} | body ${box(document.body)}`,
      `form ${box(form)} | input ${box(input)}`,
      `input gap=${inputBottom === undefined ? "—" : n(bottom - inputBottom)} outline extent=${n(outline)}`,
      `form border T/B=${formCss?.borderTopWidth ?? "—"}/${formCss?.borderBottomWidth ?? "—"} padB=${formCss?.paddingBottom ?? "—"}`,
      `focus=${isFocused} outline=${css?.outlineWidth ?? "—"} offset=${css?.outlineOffset ?? "—"}`,
      `unfocused: ${before || "—"}`,
      `focused: ${focused || "—"}`
    ].join("\n");
  };
  update();
  // Observe settled geometry as well as keyboard animation/event timing.
  window.setInterval(update, 250);
}
