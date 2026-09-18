import { useEffect, useId, useRef, useState, type PointerEvent as ReactPointerEvent, type WheelEvent } from "react";

type View = { scale: number; x: number; y: number };
type Point = { x: number; y: number };
type Gesture =
  | { kind: "pan"; point: Point; view: View }
  | { kind: "pinch"; distance: number; focal: Point; view: View };

const minimumScale = 1;
const maximumScale = 5;
const clamp = (value: number, minimum: number, maximum: number) => Math.min(maximum, Math.max(minimum, value));
const distance = (a: Point, b: Point) => Math.hypot(a.x - b.x, a.y - b.y);
const midpoint = (a: Point, b: Point): Point => ({ x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 });

export function ImageViewer({ src, name, onClose }: { src: string; name: string; onClose(): void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const surface = useRef<HTMLDivElement>(null);
  const image = useRef<HTMLImageElement>(null);
  const pointers = useRef(new Map<number, Point>());
  const gesture = useRef<Gesture | null>(null);
  const lastTap = useRef(0);
  const [view, setView] = useState<View>({ scale: 1, x: 0, y: 0 });
  const viewRef = useRef(view);
  const titleId = useId();

  useEffect(() => {
    viewRef.current = view;
  }, [view]);
  useEffect(() => {
    const element = dialog.current;
    if (element && !element.open) element.showModal();
    return () => { if (element?.open) element.close(); };
  }, []);

  const constrain = (candidate: View): View => {
    const scale = clamp(candidate.scale, minimumScale, maximumScale);
    if (scale === 1) return { scale, x: 0, y: 0 };
    const viewport = surface.current;
    const content = image.current;
    if (!viewport || !content) return { ...candidate, scale };
    const maxX = Math.max(0, (content.clientWidth * scale - viewport.clientWidth) / 2);
    const maxY = Math.max(0, (content.clientHeight * scale - viewport.clientHeight) / 2);
    return { scale, x: clamp(candidate.x, -maxX, maxX), y: clamp(candidate.y, -maxY, maxY) };
  };
  const update = (candidate: View) => setView(constrain(candidate));
  const zoom = (scale: number) => update({ ...viewRef.current, scale });
  const toggleZoom = () => zoom(viewRef.current.scale > 1 ? 1 : 2.5);

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
    event.currentTarget.setPointerCapture(event.pointerId);
    pointers.current.set(event.pointerId, { x: event.clientX, y: event.clientY });
    beginGesture();
  };
  const pointerMove = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!pointers.current.has(event.pointerId)) return;
    pointers.current.set(event.pointerId, { x: event.clientX, y: event.clientY });
    const active = [...pointers.current.values()];
    const current = gesture.current;
    if (active.length >= 2) {
      if (!current || current.kind !== "pinch") { beginGesture(); return; }
      const center = midpoint(active[0]!, active[1]!);
      const bounds = surface.current?.getBoundingClientRect();
      const viewportCenter = { x: (bounds?.left ?? 0) + (bounds?.width ?? 0) / 2, y: (bounds?.top ?? 0) + (bounds?.height ?? 0) / 2 };
      const scale = current.view.scale * distance(active[0]!, active[1]!) / current.distance;
      update({
        scale,
        x: center.x - viewportCenter.x - current.focal.x * scale,
        y: center.y - viewportCenter.y - current.focal.y * scale
      });
    } else if (active[0] && current?.kind === "pan" && current.view.scale > 1) {
      update({ scale: current.view.scale, x: current.view.x + active[0].x - current.point.x, y: current.view.y + active[0].y - current.point.y });
    }
  };
  const pointerEnd = (event: ReactPointerEvent<HTMLDivElement>) => {
    const current = gesture.current;
    const point = pointers.current.get(event.pointerId);
    const isTap = event.pointerType === "touch" && pointers.current.size === 1 && current?.kind === "pan" && point
      && distance(current.point, point) < 10;
    pointers.current.delete(event.pointerId);
    if (isTap) {
      const now = performance.now();
      if (now - lastTap.current < 320) { toggleZoom(); lastTap.current = 0; }
      else lastTap.current = now;
    }
    beginGesture();
  };
  const pointerCancel = (event: ReactPointerEvent<HTMLDivElement>) => {
    pointers.current.delete(event.pointerId);
    beginGesture();
  };
  const wheel = (event: WheelEvent<HTMLDivElement>) => {
    event.preventDefault();
    zoom(viewRef.current.scale * Math.exp(-event.deltaY * .002));
  };

  return <dialog ref={dialog} className="sp-image-viewer" aria-labelledby={titleId}
    onCancel={event => { event.preventDefault(); onClose(); }} onClose={onClose}
    onKeyDown={event => {
      if (event.key === "+" || event.key === "=") zoom(viewRef.current.scale * 1.25);
      if (event.key === "-") zoom(viewRef.current.scale / 1.25);
      if (event.key === "0") zoom(1);
    }}>
    <header className="sp-image-viewer-head">
      <h2 id={titleId}>{name}</h2>
      <button type="button" className="sp-image-viewer-control" aria-label="Lukk bilete" title="Lukk bilete" onClick={onClose}>×</button>
    </header>
    <div ref={surface} className="sp-image-viewer-surface" onPointerDown={pointerDown} onPointerMove={pointerMove}
      onPointerUp={pointerEnd} onPointerCancel={pointerCancel} onDoubleClick={toggleZoom} onWheel={wheel}>
      <img ref={image} src={src} alt={name} draggable={false}
        style={{ transform: `translate3d(${view.x}px, ${view.y}px, 0) scale(${view.scale})` }} />
    </div>
    <div className="sp-image-viewer-tools" aria-label="Zoomkontrollar">
      <button type="button" className="sp-image-viewer-control" aria-label="Zoom ut" title="Zoom ut" disabled={view.scale <= 1} onClick={() => zoom(view.scale / 1.4)}>−</button>
      <button type="button" className="sp-image-viewer-reset" onClick={() => zoom(1)} aria-label="Tilpass biletet til skjermen">{Math.round(view.scale * 100)}%</button>
      <button type="button" className="sp-image-viewer-control" aria-label="Zoom inn" title="Zoom inn" disabled={view.scale >= maximumScale} onClick={() => zoom(view.scale * 1.4)}>+</button>
    </div>
    <p className="sp-sr" aria-live="polite">Zoom {Math.round(view.scale * 100)} prosent</p>
  </dialog>;
}
