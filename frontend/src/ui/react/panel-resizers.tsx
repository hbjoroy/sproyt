import { useLayoutEffect, useRef, useState, type PointerEvent } from "react";
import { createPortal } from "react-dom";

type Panel = "sidebar" | "thread";
type Widths = Record<Panel, number | null>;
type Range = { value: number; min: number; max: number };
type Layout = {
  workspace: HTMLElement; discussion: HTMLElement; width: number; contentWidth: number;
  sidebar: Range; thread: Range; wide: boolean; split: boolean;
};
type Drag = {
  panel: Panel; pointerId: number; element: HTMLElement; startX: number;
  startWidth: number; previous: Widths;
};

const storageKey = "sproyt.panel-widths.v1";
const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, value));

function readWidths(): Widths {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(storageKey) || "null");
    if (saved && typeof saved === "object") {
      const width = (panel: Panel, min: number, max: number) => {
        const value = (saved as Record<string, unknown>)[panel];
        return typeof value === "number" && Number.isFinite(value) ? clamp(value, min, max) : null;
      };
      return { sidebar: width("sidebar", 240, 420), thread: width("thread", 340, 720) };
    }
  } catch { /* Layout still works when optional local storage is unavailable. */ }
  return { sidebar: null, thread: null };
}

/** Host layout only: resizing never republishes application state or remounts a pane. */
export function PanelResizers({ discussion, threadOpen }: {
  discussion: HTMLDivElement | null; threadOpen: boolean;
}) {
  const [layout, setLayout] = useState<Layout | null>(null);
  const current = useRef<Layout | null>(null);
  const widths = useRef<Widths>({ sidebar: null, thread: null });
  const hasThread = useRef(threadOpen);
  hasThread.current = threadOpen;
  const drag = useRef<Drag | null>(null);
  const frame = useRef(0);
  const measure = useRef<(() => void) | null>(null);

  const save = () => {
    try { localStorage.setItem(storageKey, JSON.stringify(widths.current)); } catch { /* optional */ }
  };
  const finishDrag = (cancel = false) => {
    const active = drag.current;
    if (!active) return;
    drag.current = null;
    cancelAnimationFrame(frame.current);
    frame.current = 0;
    if (cancel) widths.current = active.previous;
    current.current?.workspace.removeAttribute("data-panel-dragging");
    if (active.element.hasPointerCapture(active.pointerId)) active.element.releasePointerCapture(active.pointerId);
    measure.current?.();
    if (!cancel) save();
  };

  useLayoutEffect(() => {
    const workspace = discussion?.closest<HTMLElement>(".sp-workspace");
    if (!discussion || !workspace) return;
    widths.current = readWidths();
    measure.current = () => {
      const width = workspace.clientWidth;
      const wide = width > 900;
      // Keep both conversation panes usable whenever this screen can show them.
      const contentMinimum = hasThread.current && width >= 1020 ? 780 : 560;
      const sidebarMax = Math.max(240, Math.min(420, width - contentMinimum));
      const sidebar = { min: 240, max: sidebarMax,
        value: Math.round(clamp(widths.current.sidebar ?? clamp(width * .21, 240, 260), 240, sidebarMax)) };
      const contentWidth = wide ? width - sidebar.value : width;
      const threadMax = Math.max(340, Math.min(720, contentWidth - 440));
      const thread = { min: 340, max: threadMax,
        value: Math.round(clamp(widths.current.thread ?? contentWidth * .4, 340, threadMax)) };
      const split = wide && hasThread.current && contentWidth >= 780;
      workspace.toggleAttribute("data-resizable-panels", wide);
      workspace.style.setProperty("--sp-sidebar-width", `${sidebar.value}px`);
      discussion.toggleAttribute("data-resizable-thread", split);
      discussion.style.setProperty("--sp-thread-width", `${thread.value}px`);
      const next = { workspace, discussion, width, contentWidth, sidebar, thread, wide, split };
      current.current = next;
      setLayout(next);
    };
    const observer = new ResizeObserver(() => {
      if (drag.current && current.current?.width !== workspace.clientWidth) finishDrag();
      measure.current?.();
    });
    observer.observe(workspace);
    measure.current();
    const blur = () => finishDrag();
    const escape = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || !drag.current) return;
      // Mouse dragging preserves composer focus; intercept before a thread's
      // normal Escape action can close it instead of cancelling the resize.
      event.preventDefault();
      event.stopPropagation();
      finishDrag(true);
    };
    window.addEventListener("blur", blur);
    window.addEventListener("keydown", escape, true);
    return () => {
      observer.disconnect();
      window.removeEventListener("blur", blur);
      window.removeEventListener("keydown", escape, true);
      cancelAnimationFrame(frame.current);
      frame.current = 0;
      measure.current = null;
      const active = drag.current;
      drag.current = null;
      if (active?.element.hasPointerCapture(active.pointerId)) active.element.releasePointerCapture(active.pointerId);
      workspace.removeAttribute("data-panel-dragging");
      workspace.removeAttribute("data-resizable-panels");
      workspace.style.removeProperty("--sp-sidebar-width");
      discussion.removeAttribute("data-resizable-thread");
      discussion.style.removeProperty("--sp-thread-width");
    };
  }, [discussion]);

  useLayoutEffect(() => { finishDrag(); measure.current?.(); }, [threadOpen]);

  const change = (panel: Panel, value: number | null) => {
    const range = current.current?.[panel];
    if (!range) return;
    widths.current[panel] = value === null ? null : clamp(value, range.min, range.max);
    measure.current?.();
    save();
  };
  const start = (panel: Panel, event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0 || !event.isPrimary || !current.current || drag.current) return;
    if (current.current[panel].max <= current.current[panel].min) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = { panel, pointerId: event.pointerId, element: event.currentTarget,
      startX: event.clientX, startWidth: current.current[panel].value, previous: { ...widths.current } };
    current.current.workspace.setAttribute("data-panel-dragging", panel);
  };
  const move = (event: PointerEvent<HTMLDivElement>) => {
    const active = drag.current;
    const range = active && current.current?.[active.panel];
    if (!active || !range || active.pointerId !== event.pointerId) return;
    const direction = active.panel === "sidebar" ? 1 : -1;
    widths.current[active.panel] = clamp(active.startWidth + direction * (event.clientX - active.startX), range.min, range.max);
    if (!frame.current) frame.current = requestAnimationFrame(() => { frame.current = 0; measure.current?.(); });
  };

  if (!layout) return null;
  const handle = (panel: Panel) => {
    const range = layout[panel];
    const adjustable = range.max > range.min;
    return <div role="separator" aria-orientation="vertical" tabIndex={adjustable ? 0 : -1}
      aria-disabled={!adjustable || undefined}
      aria-label={panel === "sidebar" ? "Breidd på sidepanelet" : "Breidd på trådpanelet"}
      aria-valuemin={range.min} aria-valuemax={range.max} aria-valuenow={range.value}
      aria-valuetext={`${range.value} pikslar`}
      title="Dra for å endre breidd. Bruk piltastane, Home eller End. Dobbeltklikk eller Enter for standardbreidd."
      className="sp-panel-resizer" data-panel={panel}
      style={{ left: panel === "sidebar" ? layout.sidebar.value : layout.contentWidth - layout.thread.value }}
      onPointerDown={event => start(panel, event)} onPointerMove={move}
      onPointerUp={event => { if (drag.current?.pointerId === event.pointerId) finishDrag(); }}
      onPointerCancel={event => { if (drag.current?.pointerId === event.pointerId) finishDrag(true); }}
      onLostPointerCapture={event => { if (drag.current?.pointerId === event.pointerId) finishDrag(); }}
      onDoubleClick={() => change(panel, null)}
      onKeyDown={event => {
        const range = current.current?.[panel];
        if (!range || range.max <= range.min) return;
        const direction = panel === "sidebar" ? 1 : -1;
        const step = event.shiftKey ? 40 : 16;
        if (["ArrowLeft", "ArrowRight", "Home", "End", "Enter"].includes(event.key)) {
          event.preventDefault(); event.stopPropagation();
          if (event.key === "Enter") change(panel, null);
          else if (event.key === "Home") change(panel, range.min);
          else if (event.key === "End") change(panel, range.max);
          else change(panel, range.value + direction * (event.key === "ArrowRight" ? step : -step));
        }
      }} />;
  };
  return <>
    {layout.wide && createPortal(handle("sidebar"), layout.workspace)}
    {layout.split && createPortal(handle("thread"), layout.discussion)}
  </>;
}
