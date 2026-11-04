import { useEffect, useId, useRef, useState, type PointerEvent as ReactPointerEvent, type WheelEvent } from "react";
import { ImageDownloadLink } from "./image-download-link";

type View = { scale: number; x: number; y: number };
type Point = { x: number; y: number };
type Gesture =
  | { kind: "pan"; point: Point; view: View }
  | { kind: "pinch"; distance: number; focal: Point; view: View };

const minimumScale = .05;
const maximumScale = 5;
const clamp = (value: number, minimum: number, maximum: number) => Math.min(maximum, Math.max(minimum, value));
const distance = (a: Point, b: Point) => Math.hypot(a.x - b.x, a.y - b.y);
const midpoint = (a: Point, b: Point): Point => ({ x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 });

export function ImageViewer({ src, downloadSrc, name, onClose }: { src: string; downloadSrc: string; name: string; onClose(): void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const surface = useRef<HTMLDivElement>(null);
  const image = useRef<HTMLImageElement>(null);
  const pointers = useRef(new Map<number, Point>());
  const gesture = useRef<Gesture | null>(null);
  const tap = useRef<{ pointerId: number; point: Point; startedAt: number } | null>(null);
  const lastTap = useRef<number | null>(null);
  const lastPointerType = useRef("");
  const [view, setView] = useState<View>({ scale: 1, x: 0, y: 0 });
  const viewRef = useRef(view);
  const fittedScale = useRef(1);
  const fitting = useRef<"width" | "screen" | null>("width");
  const [ready, setReady] = useState(false);
  const titleId = useId();

  useEffect(() => {
    const element = dialog.current;
    if (element && !element.open) element.showModal();
    return () => { if (element?.open) element.close(); };
  }, []);

  const limitScale = (scale: number) => clamp(scale, Math.min(minimumScale, fittedScale.current), maximumScale);
  const constrain = (candidate: View): View => {
    const scale = limitScale(candidate.scale);
    const viewport = surface.current;
    const content = image.current;
    if (!viewport || !content) return { ...candidate, scale };
    const maxX = Math.max(0, (content.naturalWidth * scale - viewport.clientWidth) / 2);
    const maxY = Math.max(0, (content.naturalHeight * scale - viewport.clientHeight) / 2);
    return { scale, x: clamp(candidate.x, -maxX, maxX), y: clamp(candidate.y, -maxY, maxY) };
  };
  const update = (candidate: View) => {
    const next = constrain(candidate);
    viewRef.current = next;
    setView(current => current.scale === next.scale && current.x === next.x && current.y === next.y ? current : next);
  };
  const zoom = (scale: number) => { fitting.current = null; update({ ...viewRef.current, scale }); };
  const fit = () => { fitting.current = "screen"; update({ scale: fittedScale.current, x: 0, y: 0 }); };
  const actualSize = () => { fitting.current = null; update({ scale: 1, x: 0, y: 0 }); };
  const toggleZoom = () => {
    if (fitting.current === null && viewRef.current.scale > fittedScale.current * 1.01) fit();
    else zoom(fittedScale.current < 1 ? 1 : 2.5);
  };
  const measure = () => {
    const viewport = surface.current;
    const content = image.current;
    if (!viewport?.clientWidth || !viewport.clientHeight || !content?.naturalWidth || !content.naturalHeight) return;
    // 100% is one decoded original pixel per CSS pixel, independent of viewport size.
    fittedScale.current = Math.min(1, viewport.clientWidth / content.naturalWidth, viewport.clientHeight / content.naturalHeight);
    const scale = fitting.current === "width" ? Math.min(1, viewport.clientWidth / content.naturalWidth)
      : fitting.current === "screen" ? fittedScale.current : viewRef.current.scale;
    update({ ...viewRef.current, scale });
    setReady(true);
    // A resized image/viewport invalidates a drag's old coordinates.
    pointers.current.clear();
    gesture.current = null;
    tap.current = null;
    lastTap.current = null;
  };

  useEffect(() => {
    fitting.current = "width";
    setReady(false);
    update({ scale: 1, x: 0, y: 0 });
    const observer = new ResizeObserver(measure);
    if (surface.current) observer.observe(surface.current);
    if (image.current) observer.observe(image.current);
    if (image.current?.complete) measure();
    return () => observer.disconnect();
  }, [src]);

  const beginGesture = () => {
    const active = [...pointers.current.values()];
    if (active.length >= 2) {
      const center = midpoint(active[0]!, active[1]!);
      const bounds = surface.current?.getBoundingClientRect();
      const viewportCenter = { x: (bounds?.left ?? 0) + (bounds?.width ?? 0) / 2, y: (bounds?.top ?? 0) + (bounds?.height ?? 0) / 2 };
      gesture.current = {
        kind: "pinch", distance: Math.max(1, distance(active[0]!, active[1]!)), view: viewRef.current,
        focal: {
          x: (center.x - viewportCenter.x - viewRef.current.x) / viewRef.current.scale,
          y: (center.y - viewportCenter.y - viewRef.current.y) / viewRef.current.scale
        }
      };
    }
    else if (active[0]) gesture.current = { kind: "pan", point: active[0], view: viewRef.current };
    else gesture.current = null;
  };
  const pointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    lastPointerType.current = event.pointerType;
    event.currentTarget.setPointerCapture(event.pointerId);
    pointers.current.set(event.pointerId, { x: event.clientX, y: event.clientY });
    if (event.pointerType === "touch" && pointers.current.size === 1) {
      tap.current = { pointerId: event.pointerId, point: { x: event.clientX, y: event.clientY }, startedAt: performance.now() };
    } else {
      tap.current = null;
      lastTap.current = null;
    }
    beginGesture();
  };
  const pointerMove = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!pointers.current.has(event.pointerId)) return;
    pointers.current.set(event.pointerId, { x: event.clientX, y: event.clientY });
    if (tap.current?.pointerId === event.pointerId && distance(tap.current.point, { x: event.clientX, y: event.clientY }) >= 10) {
      tap.current = null;
      lastTap.current = null;
    }
    const active = [...pointers.current.values()];
    const current = gesture.current;
    if (active.length >= 2) {
      if (!current || current.kind !== "pinch") { beginGesture(); return; }
      const center = midpoint(active[0]!, active[1]!);
      const bounds = surface.current?.getBoundingClientRect();
      const viewportCenter = { x: (bounds?.left ?? 0) + (bounds?.width ?? 0) / 2, y: (bounds?.top ?? 0) + (bounds?.height ?? 0) / 2 };
      const scale = limitScale(current.view.scale * distance(active[0]!, active[1]!) / current.distance);
      fitting.current = null;
      update({
        scale,
        x: center.x - viewportCenter.x - current.focal.x * scale,
        y: center.y - viewportCenter.y - current.focal.y * scale
      });
    } else if (active[0] && current?.kind === "pan") {
      update({ scale: current.view.scale, x: current.view.x + active[0].x - current.point.x, y: current.view.y + active[0].y - current.point.y });
    }
  };
  const pointerEnd = (event: ReactPointerEvent<HTMLDivElement>) => {
    const now = performance.now();
    const isTap = event.pointerType === "touch" && tap.current?.pointerId === event.pointerId
      && now - tap.current.startedAt < 320
      && distance(tap.current.point, { x: event.clientX, y: event.clientY }) < 10;
    tap.current = null;
    pointers.current.delete(event.pointerId);
    if (isTap) {
      if (lastTap.current !== null && now - lastTap.current < 320) { toggleZoom(); lastTap.current = null; }
      else lastTap.current = now;
    } else lastTap.current = null;
    beginGesture();
  };
  const pointerCancel = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!pointers.current.has(event.pointerId)) return;
    tap.current = null;
    lastTap.current = null;
    pointers.current.delete(event.pointerId);
    beginGesture();
  };
  const wheel = (event: WheelEvent<HTMLDivElement>) => {
    event.preventDefault();
    zoom(viewRef.current.scale * Math.exp(-event.deltaY * .002));
  };
  const percentage = new Intl.NumberFormat(["nn-NO", "nb-NO"], { maximumFractionDigits: view.scale < .01 ? 2 : 1 }).format(view.scale * 100);

  return <dialog ref={dialog} className="sp-image-viewer" aria-labelledby={titleId}
    onCancel={event => { event.preventDefault(); onClose(); }} onClose={onClose}
    onKeyDown={event => {
      if (!ready || event.ctrlKey || event.metaKey || event.altKey) return;
      if (event.key === "+" || event.key === "=") zoom(viewRef.current.scale * 1.25);
      if (event.key === "-") zoom(viewRef.current.scale / 1.25);
      if (event.key === "0") actualSize();
    }}>
    <header className="sp-image-viewer-head">
      <h2 id={titleId}>{name}</h2>
      <div className="sp-image-viewer-actions">
        <ImageDownloadLink className="sp-image-viewer-control" href={downloadSrc} name={name} />
        <button type="button" className="sp-image-viewer-control" aria-label="Lukk bilete" title="Lukk bilete" onClick={onClose}>×</button>
      </div>
    </header>
    <div ref={surface} className="sp-image-viewer-surface" onContextMenu={event => event.preventDefault()} onPointerDown={pointerDown} onPointerMove={pointerMove}
      onPointerUp={pointerEnd} onPointerCancel={pointerCancel} onLostPointerCapture={pointerCancel}
      // Touch double taps are already handled on pointerup; browsers may also emit dblclick.
      onDoubleClick={() => { if (lastPointerType.current !== "touch") toggleZoom(); }} onWheel={wheel}>
      <img ref={image} src={src} alt={name} draggable={false}
        onLoad={measure} style={{ visibility: ready ? "visible" : "hidden", transform: `translate3d(${view.x}px, ${view.y}px, 0) scale(${view.scale})` }} />
    </div>
    <div className="sp-image-viewer-tools" aria-label="Zoomkontrollar">
      <button type="button" className="sp-image-viewer-control" aria-label="Zoom ut" title="Zoom ut" disabled={!ready || view.scale <= Math.min(minimumScale, fittedScale.current)} onClick={() => zoom(view.scale / 1.4)}>-</button>
      <button type="button" className="sp-image-viewer-reset" onClick={fit} disabled={!ready} aria-label="Tilpass biletet til skjermen" title="Tilpass biletet til skjermen">{percentage}%</button>
      <button type="button" className="sp-image-viewer-reset" onClick={actualSize} disabled={!ready} aria-label="Vis biletet i faktisk storleik (100 %)" title="Faktisk storleik (100 %)">1:1</button>
      <button type="button" className="sp-image-viewer-control" aria-label="Zoom inn" title="Zoom inn" disabled={!ready || view.scale >= maximumScale} onClick={() => zoom(view.scale * 1.4)}>+</button>
    </div>
    <p className="sp-sr" aria-live="polite">Zoom {percentage} prosent</p>
  </dialog>;
}
