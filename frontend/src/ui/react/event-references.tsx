import { createContext, useContext, useState, type ReactNode } from "react";
import type { ChannelEvent } from "../../channel-events";

export type EventReferences = Readonly<{
  events: readonly ChannelEvent[];
  collect?(codes: ReadonlySet<string>): void;
  href(event: ChannelEvent): string;
  open(event: ChannelEvent): void;
}>;
export const EventReferencesContext = createContext<EventReferences | null>(null);
export const useEventReferences = () => useContext(EventReferencesContext);
export const EventBridgeContext = createContext<{
  publish(references: Pick<EventReferences, "events" | "collect">): void;
  request: { id: string } | null;
} | null>(null);

/** Share the active channel's already-authorized discovery with its thread pane. */
export function ChannelEventBoundary({ children, href }: { children: ReactNode; href(event: ChannelEvent): string }) {
  const [references, setReferences] = useState<Pick<EventReferences, "events" | "collect">>({ events: [] });
  const [request, setRequest] = useState<{ id: string } | null>(null);
  return <EventBridgeContext.Provider value={{ publish: setReferences, request }}>
    <EventReferencesContext.Provider value={{ ...references, href, open: event => setRequest({ id: event.id }) }}>{children}</EventReferencesContext.Provider>
  </EventBridgeContext.Provider>;
}

type Node = { type: string; value?: string; children?: Node[]; url?: string; data?: { hProperties: Record<string, string> };
  position?: { start: { offset?: number }; end: { offset?: number } } };
const literal = new Set(["link", "linkReference", "image", "imageReference", "inlineCode", "code", "html"]);
/** Runs before channel discovery: #arr:code must never become a #arr channel. */
export function eventReferencePlugin(source: string, references: EventReferences, candidates?: Set<string>) {
  function transform(node: Node) {
    if (!node.children || literal.has(node.type)) return;
    node.children = node.children.flatMap(child => {
      if (child.type !== "text" || !child.value) { transform(child); return [child]; }
      const text = child.value;
      const raw = source.slice(child.position?.start.offset, child.position?.end.offset);
      const rawTokens = [...raw.matchAll(/(?:^|[\s([{])(\\*)#arr:([a-z0-9-]{1,48})(?![\p{L}\p{N}_&-])/giu)];
      let rawCursor = 0;
      const result: Node[] = [];
      let end = 0;
      for (const match of text.matchAll(/(^|[\s([{])#arr:([a-z0-9-]{1,48})(?![\p{L}\p{N}_-])/giu)) {
        const rawAt = rawTokens.findIndex((token, index) => index >= rawCursor && token[2]?.toLowerCase() === match[2]!.toLowerCase());
        if (rawAt < 0) continue;
        if (rawAt >= 0) { rawCursor = rawAt + 1; if (rawTokens[rawAt]![1]!.length > 0) continue; }
        if (candidates && candidates.size < 50) candidates.add(match[2]!.toLowerCase());
        const event = references.events.find(event => event.code === match[2]!.toLowerCase() || event.aliases.includes(match[2]!.toLowerCase()));
        if (!event) continue;
        const start = match.index + match[1]!.length;
        if (start > end) result.push({ type: "text", value: text.slice(end, start) });
        result.push({ type: "link", url: references.href(event), data: { hProperties: { "data-event-reference": event.id } },
          children: [{ type: "text", value: text.slice(start, match.index + match[0].length) }] });
        end = match.index + match[0].length;
      }
      if (!end) return [child];
      if (end < text.length) result.push({ type: "text", value: text.slice(end) });
      return result;
    });
  }
  return () => transform;
}
