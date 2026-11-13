import { unified } from "unified";
import remarkParse from "remark-parse";
import remarkGfm from "remark-gfm";
import { eventReferencePlugin } from "./event-references";
import type { ChannelEvent } from "../../channel-events";

const parser = unified().use(remarkParse).use(remarkGfm);
export function eventContextWhen(event: ChannelEvent): string {
  try {
    const format = new Intl.DateTimeFormat(["nb-NO", "en-GB"], { dateStyle: "medium", timeStyle: "short", timeZone: event.timezone, hourCycle: "h23" });
    return `${format.format(new Date(event.starts_at))} · ${event.timezone}`;
  } catch { return `${event.starts_at} · ${event.timezone}`; }
}
/** Use the display parser so escaped text, code and links are never mistaken for a reference. */
export function messageWithEventContext(source: string, event: ChannelEvent): string {
  const codes = new Set<string>();
  eventReferencePlugin(source, { events: [], href: () => "", open() {} }, codes)()(parser.parse(source));
  if ([event.code, ...event.aliases].some(code => codes.has(code))) return source;
  if (codes.size >= 50) throw new Error("Meldinga har allereie 50 arrangementsreferansar. Fjern ein referanse eller den valde arrangementskonteksten før du sender.");
  return `${source}${source ? "\n\n" : ""}#arr:${event.code}`;
}
