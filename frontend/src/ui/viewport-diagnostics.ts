/** Opt-in, on-device geometry only: no draft/message contents or network calls. */
export function installViewportDiagnostics(): void {
  if (!new URLSearchParams(location.search).has("viewport-debug")) return;
  const panel = document.createElement("pre");
  panel.id = "sproyt-viewport-diagnostics";
  panel.setAttribute("aria-label", "Viewport diagnostics");
  Object.assign(panel.style, {
    position: "fixed", left: "4px", right: "4px", zIndex: "2147483647",
    margin: "0", padding: "6px", border: "1px solid #888", background: "#111e",
    color: "#fff", font: "10px/1.25 monospace", whiteSpace: "pre-wrap",
    pointerEvents: "none", overflow: "hidden"
  });
  // A fixed, zero-width, hidden probe resolves viewport units and safe-area
  // variables without participating in layout, painting, focus, or hit testing.
  const probe = document.createElement("div");
  probe.setAttribute("aria-hidden", "true");
  probe.setAttribute("inert", "");
  Object.assign(probe.style, {
    position: "fixed", inset: "0 auto auto 0", width: "0", height: "100dvh",
    paddingTop: "env(safe-area-inset-top, 0px)", paddingBottom: "env(safe-area-inset-bottom, 0px)",
    visibility: "hidden", pointerEvents: "none", contain: "strict", overflow: "hidden"
  });
  document.body.append(probe);
  document.body.append(panel);
  const n = (value: number) => value.toFixed(1);
  const label = (element: Element | null | undefined) => element
    ? `${element.tagName.toLowerCase()}${element.classList.length ? `.${[...element.classList].slice(0, 3).join(".")}` : ""}`
    : "—";
  let before = "";
  let focused = "";
  let pointerCount = 0;
  let pointerTarget = "—";
  document.addEventListener("pointerdown", event => {
    pointerCount += 1;
    pointerTarget = event.target instanceof Element ? label(event.target) : "—";
  }, { capture: true, passive: true });
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
      return `${n(rect.top)}..${n(rect.bottom)}`;
    };
    const css = input && getComputedStyle(input);
    const formCss = form && getComputedStyle(form);
    const bodyCss = getComputedStyle(document.body);
    const probeCss = getComputedStyle(probe);
    const inputBottom = input?.getBoundingClientRect().bottom;
    const outline = css && css.outlineStyle !== "none"
      ? parseFloat(css.outlineWidth) + parseFloat(css.outlineOffset) : 0;
    const isFocused = document.activeElement?.tagName === "TEXTAREA";
    const inputCenter = input && input.getBoundingClientRect();
    const hit = inputCenter && document.elementFromPoint(
      inputCenter.left + inputCenter.width / 2,
      inputCenter.top + inputCenter.height / 2
    );
    const hitLabel = label(hit);
    const displayMode = ["fullscreen", "standalone", "minimal-ui"]
      .find(mode => matchMedia(`(display-mode: ${mode})`).matches) ?? "browser";
    const inputInert = Boolean(input?.closest("[inert]"));
    const summary = `vv=${n(viewport?.height ?? innerHeight)} top=${n(top)} app=${root ? n(root.getBoundingClientRect().bottom) : "—"}`;
    if (isFocused) focused = summary;
    else before = summary;
    panel.style.top = `${top + 4}px`;
    panel.textContent = [
      "Viewport diagnostic v2 (CSS px)",
      `vv ${n(viewport?.height ?? innerHeight)} ${n(top)}..${n(bottom)} s=${n(viewport?.scale ?? 1)} i/c=${n(innerHeight)}/${document.documentElement.clientHeight}`,
      `app viewport=${document.documentElement.dataset.appViewport ?? "—"} scrollY=${n(scrollY)}`,
      `dvh=${probeCss.height} safe T/B=${probeCss.paddingTop}/${probeCss.paddingBottom} mode=${displayMode}`,
      `app ${box(root)} body ${box(document.body)}`,
      `doc C/S=${document.documentElement.clientHeight}/${document.documentElement.scrollHeight} body C/S=${document.body.clientHeight}/${document.body.scrollHeight}`,
      `form ${box(form)} input ${box(input)}`,
      `gap=${inputBottom === undefined ? "—" : n(bottom - inputBottom)} outline extent=${n(outline)} form padB=${formCss?.paddingBottom ?? "—"}`,
      `focus=${isFocused} disabled=${input?.disabled ?? "—"} inert=${inputInert} dlg=${document.querySelectorAll("dialog[open]").length} hit=${hitLabel}`,
      `pointer=${pointerCount}:${pointerTarget}`,
      `body ${bodyCss.position} h=${bodyCss.height}`,
      `unfocused ${before || "—"}`,
      `focused ${focused || "—"}`
    ].join("\n");
  };
  update();
  // Observe settled geometry as well as keyboard animation/event timing.
  window.setInterval(update, 250);
}
