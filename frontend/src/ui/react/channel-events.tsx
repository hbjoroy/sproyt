import { createContext, useCallback, useContext, useEffect, useId, useMemo, useRef, useState, type ReactNode } from "react";
import { Button, Status } from "@sproyt/ui/react";
import { HttpError } from "../../api";
import { EventReferenceDiscovery, eventResponses, type ChannelEvent, type ChannelEventsApi, type EventCollectionSummary, type EventInput, type EventMessages, type EventPerson, type EventResponse } from "../../channel-events";
import { ConversationTimeline, type TimelineProps } from "./conversation-view";
import { composeEventInChat, EventBridgeContext, EventReferencesContext } from "./event-references";
import { MarkdownContent } from "./markdown-content";
import { ImageViewer } from "./image-viewer";
import { PreviewImage } from "./preview-media";
import css from "./channel-events.css";

/** Event and topic filters share presentation while retaining their own result sources. */
export const ChannelEventFilterContext = createContext<{
  selected: boolean; available: boolean; clear(): void; controls: ReactNode; results: ReactNode;
} | null>(null);

export const responseLabels: Record<EventResponse, string> = { attending: "Deltek", declined: "Deltek ikkje", following: "Følgjer", unanswered: "Ikkje svart" };
type Scope = { api: ChannelEventsApi; owner: string; channelId: string };
const denied = (error: unknown) => error instanceof HttpError && [401, 403, 404].includes(error.status);
const errorText = (error: unknown) => error instanceof Error ? error.message : "Prøv igjen.";
export function eventHref(channelId: string, id: string) {
  const url = new URL(window.location.href);
  for (const key of ["view", "thread", "message", "sequence"]) url.searchParams.delete(key);
  url.searchParams.set("channel", channelId); url.searchParams.set("event", id);
  return `${url.pathname}${url.search}${url.hash}`;
}
function mediaUrl(id: string, original = false) {
  const participant = new URL(window.location.href).searchParams.get("participant");
  return `/api/v1/media/${encodeURIComponent(id)}${original ? "" : "/preview"}${participant ? `?participant=${encodeURIComponent(participant)}` : ""}`;
}
export function eventWhen(event: ChannelEvent) {
  try {
    const format = new Intl.DateTimeFormat(["nb-NO", "en-GB"], { dateStyle: "medium", timeStyle: "short", timeZone: event.timezone, hourCycle: "h23" });
    return `${format.format(new Date(event.starts_at))}${event.ends_at ? ` – ${format.format(new Date(event.ends_at))}` : ""} · ${event.timezone}`;
  } catch { return `${event.starts_at} · ${event.timezone}`; }
}

/** Both panes stay mounted: drafts, attachment state and timeline scroll survive tab switches. */
export function ChannelEventSpace({ api, owner, channelId, enabled: channelEnabled, chat, composer, timeline, uploadImage, onTabChange }: Scope & {
  enabled: boolean;
  chat: ReactNode; composer?: ReactNode; timeline: TimelineProps; uploadImage(file: File, progress: (text: string) => void): Promise<string>;
  onTabChange?(events: boolean): void;
}) {
  const bridge = useContext(EventBridgeContext);
  // HTTP can observe a moderator change before the channel list. Fresh bounded
  // reads may restore an explicit re-enable without remounting the chat.
  const [serverDisabled, setServerDisabled] = useState(false);
  const enabled = channelEnabled && !serverDisabled;
  const previousChannelEnabled = useRef(channelEnabled);
  useEffect(() => {
    if (channelEnabled && !previousChannelEnabled.current) setServerDisabled(false);
    previousChannelEnabled.current = channelEnabled;
  }, [channelEnabled]);
  const initial = () => {
    const url = new URL(window.location.href);
    return url.searchParams.get("channel") === channelId ? url.searchParams.get("event") : null;
  };
  const [selectedId, setSelectedId] = useState<string | null>(initial);
  const [tab, setTab] = useState<"chat" | "events">(() => initial() ? "events" : "chat");
  const [events, setEvents] = useState<readonly ChannelEvent[]>([]);
  const eventsRef = useRef(events); eventsRef.current = events;
  const [attentionCount, setAttentionCount] = useState<number | null>(null);
  const [summary, setSummary] = useState<EventCollectionSummary | null>(null);
  const [summaryError, setSummaryError] = useState(false);
  const [accessDenied, setAccessDenied] = useState(false);
  const referencesAllowed = enabled && !accessDenied;
  const [referenceVersion, setReferenceVersion] = useState(0);
  const discovery = useMemo(() => new EventReferenceDiscovery(api, owner, channelId, () => setReferenceVersion(value => value + 1)), [api, owner, channelId]);
  useEffect(() => () => discovery.dispose(), [discovery]);
  const collect = useCallback((codes: ReadonlySet<string>) => discovery.collect(codes), [discovery]);
  const search = useCallback((query: string, signal: AbortSignal) => api.search(owner, channelId, query, signal), [api, owner, channelId]);
  const suggest = useCallback((prefix: string, signal: AbortSignal) => api.suggestions(owner, channelId, prefix, signal), [api, owner, channelId]);
  const referenceEvents = useMemo(() => [...new Map([...discovery.events(), ...events].map(event => [event.id, event])).values()], [events, discovery, referenceVersion]);
  const [nextAfter, setNextAfter] = useState<string | null>(null);
  const [after, setAfter] = useState<string | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState("");
  const [loaded, setLoaded] = useState(false);
  const [revision, setRevision] = useState(0);
  const [editing, setEditing] = useState<ChannelEvent | "new" | null>(null);
  const [filter, setFilter] = useState<string | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const id = useId();
  const current = useRef(true);
  const previousEnabled = useRef(enabled);
  const [disabledNotice, setDisabledNotice] = useState(false);
  const reload = () => setRevision(value => value + 1);
  useEffect(() => { bridge?.publish({ events: referenceEvents, collect, search: referencesAllowed ? search : undefined, suggest: referencesAllowed ? suggest : undefined }); }, [referenceEvents, collect, suggest, search, referencesAllowed, bridge?.publish]);
  useEffect(() => {
    if (previousEnabled.current && !enabled) {
      if (tab === "events" || filter || editing) setDisabledNotice(true);
      setTab("chat"); setFilter(null); setEditing(null); setSelectedId(null); updateUrl(null);
      setAfter(null); setNextAfter(null); setEvents([]); setSummary(null); setAttentionCount(null); setSummaryError(false); setLoaded(false); setError("");
      if (document.activeElement === document.body) requestAnimationFrame(() => root.current?.querySelector<HTMLTextAreaElement>(".sp-composer textarea")?.focus({ preventScroll: true }));
    }
    previousEnabled.current = enabled;
  }, [enabled]);
  useEffect(() => {
    const event = referenceEvents.find(event => event.id === bridge?.request?.id);
    if (event) open(event);
  }, [bridge?.request]);
  useEffect(() => { current.current = true; return () => { current.current = false; }; }, []);
  useEffect(() => {
    if (!channelEnabled) return;
    const controller = new AbortController();
    setLoadingMore(true);
    void api.list(owner, channelId, controller.signal, after).then(value => {
      if (!controller.signal.aborted) { if (value.events_enabled !== undefined) setServerDisabled(!value.events_enabled); setEvents(previous => after ? [...new Map([...previous, ...value.events].map(event => [event.id, event])).values()] : value.events); setAttentionCount(value.attention_count); setSummary(value.summary); setSummaryError(false); setAccessDenied(false); setNextAfter(value.next_after); setLoaded(true); setLoadingMore(false); setError(""); }
    }, cause => {
      if (controller.signal.aborted) return;
      if (denied(cause)) { setAccessDenied(true); setEvents([]); setSummary(null); setAttentionCount(null); discovery.dispose(); setReferenceVersion(value => value + 1); setEditing(null); }
      setSummaryError(true); setLoaded(true); setError(`Arrangementa kunne ikkje lastast. ${errorText(cause)}`);
      setLoadingMore(false);
    });
    return () => controller.abort();
  }, [api, owner, channelId, revision, after, channelEnabled]);
  useEffect(() => {
    if (!channelEnabled) return;
    // Keep the compact navigation current in chat too. A bounded first-page
    // read refreshes aggregates without discarding explicitly loaded history.
    let controller: AbortController | undefined;
    const refresh = () => {
      if (document.visibilityState !== "visible") return;
      if (after === null) { reload(); return; }
      controller?.abort(); controller = new AbortController();
      const signal = controller.signal;
      void api.list(owner, channelId, signal).then(value => {
        if (!signal.aborted) { if (value.events_enabled !== undefined) setServerDisabled(!value.events_enabled); setSummary(value.summary); setAttentionCount(value.attention_count); setSummaryError(false); }
      }, cause => {
        if (!signal.aborted) { setSummaryError(true); if (denied(cause)) revoke(); }
      });
    };
    const timer = window.setInterval(refresh, 30_000);
    document.addEventListener("visibilitychange", refresh);
    window.addEventListener("focus", refresh);
    return () => { clearInterval(timer); controller?.abort(); document.removeEventListener("visibilitychange", refresh); window.removeEventListener("focus", refresh); };
  }, [api, owner, channelId, after, channelEnabled]);
  useEffect(() => { onTabChange?.(tab === "events"); return () => onTabChange?.(false); }, [tab]);
  useEffect(() => {
    const pop = () => { const event = initial(); setSelectedId(event); setTab(event ? "events" : "chat"); setEditing(null); };
    window.addEventListener("popstate", pop); return () => window.removeEventListener("popstate", pop);
  }, [channelId]);
  function updateUrl(eventId: string | null) {
    const url = new URL(window.location.href);
    if (eventId) url.searchParams.set("event", eventId); else url.searchParams.delete("event");
    window.history.replaceState(window.history.state, "", url);
  }
  function closeEditor() {
    setEditing(null);
    requestAnimationFrame(() => {
      // The editor replaces its origin. Restore a useful keyboard destination
      // after that origin is mounted again.
      root.current?.querySelector<HTMLElement>(selectedId ? ".sp-event-detail h2" : ".sp-event-list-create")?.focus({ preventScroll: true });
    });
  }
  function openOverview() { setSelectedId(null); setTab("events"); setEditing(null); updateUrl(null); }
  function open(event: Pick<ChannelEvent, "id">) {
    setSelectedId(event.id); setTab("events"); setEditing(null); updateUrl(event.id);
    requestAnimationFrame(() => root.current?.querySelector<HTMLElement>(".sp-events-panel h2")?.focus());
  }
  function upsert(event: ChannelEvent) {
    const previous = eventsRef.current.find(item => item.id === event.id);
    if (previous && previous.needs_attention !== event.needs_attention) setAttentionCount(count => count === null ? null : Math.max(0, count + (event.needs_attention ? 1 : -1)));
    setEvents(previous => [event, ...previous.filter(item => item.id !== event.id)]);
    if (previous && ["cancelled", "starts_at", "ends_at", "title"].some(key => previous[key as keyof ChannelEvent] !== event[key as keyof ChannelEvent])) {
      void api.list(owner, channelId, new AbortController().signal).then(value => {
        if (current.current) { if (value.events_enabled !== undefined) setServerDisabled(!value.events_enabled); setSummary(value.summary); setAttentionCount(value.attention_count); setSummaryError(false); }
      }, cause => { if (current.current) { setSummaryError(true); if (denied(cause)) revoke(); } });
    }
  }
  function revoke() { setAccessDenied(true); setEvents([]); setSummary(null); setAttentionCount(null); setSummaryError(true); discovery.dispose(); setReferenceVersion(value => value + 1); setEditing(null); }
  const attention = attentionCount;
  const upcoming = summary?.upcoming_count;
  const next = summary?.next_event;
  const ongoing = Boolean(next && next.ends_at && Date.parse(next.starts_at) <= Date.parse(summary!.as_of) && Date.parse(next.ends_at) > Date.parse(summary!.as_of));
  const summaryLabel = ["Arrangement", upcoming === undefined ? (summaryError ? "tal ukjent, kunne ikkje oppdaterast" : "tal ukjent") : `${upcoming} komande arrangement`,
    summary?.ongoing_count ? `${summary.ongoing_count} pågåande arrangement` : "", attention ? `${attention} arrangement endra` : "", summaryError && summary ? "kunne ikkje oppdaterast" : ""].filter(Boolean).join(", ");
  const activeFilter = enabled ? filter : null;
  return <EventReferencesContext.Provider value={{ events: referenceEvents, collect, search: referencesAllowed ? search : undefined, suggest: referencesAllowed ? suggest : undefined, href: event => eventHref(channelId, event.id), open }}>
    <style>{css}</style>
    <div className="sp-channel-events" ref={root}>
      {enabled && <div className="sp-channel-navigation"><div className="sp-channel-tabs" role="tablist" aria-label="Innhald i kanalen"
        onKeyDownCapture={event => { event.currentTarget.dataset.keyboardNav = "true"; }}
        onPointerDownCapture={event => { delete event.currentTarget.dataset.keyboardNav; }}
        onBlurCapture={event => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) delete event.currentTarget.dataset.keyboardNav; }}>
        <button type="button" id={`${id}-chat-tab`} role="tab" tabIndex={tab === "chat" ? 0 : -1} aria-selected={tab === "chat"} aria-controls={`${id}-chat`} onClick={() => { setTab("chat"); updateUrl(null); }}
          onKeyDown={event => { if (["ArrowRight", "ArrowLeft"].includes(event.key)) { event.preventDefault(); openOverview(); root.current?.querySelector<HTMLElement>(`[id="${CSS.escape(id)}-events-tab"]`)?.focus(); } }}>Meldingar</button>
        <button type="button" id={`${id}-events-tab`} role="tab" tabIndex={tab === "events" ? 0 : -1} aria-selected={tab === "events"} aria-controls={`${id}-events`} aria-label={summaryLabel} title={summaryLabel} onClick={openOverview}
          onKeyDown={event => { if (["ArrowRight", "ArrowLeft"].includes(event.key)) { event.preventDefault(); setTab("chat"); updateUrl(null); root.current?.querySelector<HTMLElement>(`[id="${CSS.escape(id)}-chat-tab"]`)?.focus(); } }}>Arrangement{summary && summary.ongoing_count > 0 && <span className="sp-events-ongoing" aria-hidden="true"> · Pågår</span>}{upcoming !== undefined && upcoming > 0 && <span className="sp-events-count" aria-hidden="true"> · {upcoming}<span className="sp-events-count-label"> komande</span></span>}{attention !== null && attention > 0 && <span className="sp-events-attention" aria-hidden="true" title={`${attention} arrangement endra`}> · {attention} endra</span>}{summaryError && <span aria-hidden="true"> · ?</span>}</button>
      </div>{next && <button type="button" className="sp-events-next" onClick={() => open(next)} title={`${ongoing ? "Pågår" : "Neste"}: ${next.title}`}>
        <span>{ongoing ? "Pågår" : `Neste: ${new Intl.DateTimeFormat("nb-NO", { day: "numeric", month: "short", timeZone: next.timezone }).format(new Date(next.starts_at))}`}</span><span> · {next.title}</span>
      </button>}</div>}
      <div className="sp-event-chat" id={`${id}-chat`} role={enabled ? "tabpanel" : undefined} aria-labelledby={enabled ? `${id}-chat-tab` : undefined} hidden={tab !== "chat"} inert={tab !== "chat"}>
        {disabledNotice && <Status>Arrangement er slått av i denne kanalen. Meldingar og utkast er bevarte. <Button variant="quiet" onClick={() => setDisabledNotice(false)}>Lukk</Button></Status>}
        <ChannelEventFilterContext.Provider value={{
          selected: Boolean(activeFilter), available: enabled && events.length > 0, clear: () => setFilter(null),
          controls: enabled ? <EventPills events={events} selected={filter} onSelect={setFilter} more={nextAfter !== null} loading={loadingMore} onLoadMore={() => setAfter(nextAfter)} /> : null,
          results: activeFilter ? <EventMessageResults key={activeFilter} api={api} owner={owner} channelId={channelId} eventId={activeFilter} timeline={timeline} onDenied={() => { revoke(); setFilter(null); }} /> : null
        }}>{chat}</ChannelEventFilterContext.Provider>
        {composer}
      </div>
      <section className="sp-events-panel" id={`${id}-events`} role={enabled ? "tabpanel" : "region"} aria-label={enabled ? undefined : "Arkivert arrangement"} aria-labelledby={enabled ? `${id}-events-tab` : undefined} hidden={tab !== "events"} inert={tab !== "events"}>
        <div className="sp-events-inner">
          {error && <Status tone="error">{error} <Button onClick={reload}>Prøv igjen</Button></Status>}
          {!loaded && enabled && <Status>Lastar arrangement …</Status>}
          {editing && enabled ? <EventEditor key={editing === "new" ? "new" : editing.id} event={editing === "new" ? undefined : editing}
            onClose={closeEditor} onSave={async (input, requestId) => {
              const created = await api.save(owner, channelId, input, editing === "new" ? undefined : editing.id, requestId);
              if (!current.current) return;
              upsert(created); setEditing(null); open(created); reload();
            }} /> : selectedId ? <EventDetail key={selectedId} api={api} owner={owner} channelId={channelId} eventId={selectedId}
              refresh={revision} onUpdate={upsert} onEdit={setEditing} onNew={() => setEditing("new")} uploadImage={uploadImage} readOnly={!enabled}
              onConversation={(event, compose) => { setFilter(event.id); setTab("chat"); updateUrl(null); if (compose) composeEventInChat(event); requestAnimationFrame(() => root.current?.querySelector<HTMLTextAreaElement>(".sp-composer textarea")?.focus({ preventScroll: true })); }}
              onDenied={revoke} onBack={() => { setSelectedId(null); if (!enabled) setTab("chat"); updateUrl(null); }} /> : <>
            <div className="sp-event-actions"><h2 tabIndex={-1}>Arrangement</h2><Button className="sp-event-list-create" variant="primary" onClick={() => setEditing("new")} disabled={Boolean(error)}>Nytt arrangement</Button>
              {after !== null && <Button onClick={() => { setAfter(null); reload(); }}>Oppdater arrangement</Button>}</div>
            <p>Alle i kanalen kan opprette arrangement og bidra med bilete.</p>
            {loaded && !events.length && !error && <p>Ingen arrangement enno. Lag det første og inviter kanalen.</p>}
            {(["Komande", "Tidlegare", "Avlyste"] as const).map(group => {
              const items = events.filter(event => group === "Avlyste" ? event.cancelled : !event.cancelled && (Date.parse(event.ends_at ?? event.starts_at) < Date.now()) === (group === "Tidlegare"))
                .sort((a, b) => group === "Komande" ? Date.parse(a.starts_at) - Date.parse(b.starts_at) : Date.parse(b.starts_at) - Date.parse(a.starts_at));
              return items.length ? <section key={group} aria-label={group}><h3>{group}</h3><ul className="sp-event-list">{items.map(event => <li key={event.id}>
                <h3><a href={eventHref(channelId, event.id)} onClick={action => { if (!action.ctrlKey && !action.metaKey && !action.shiftKey && action.button === 0) { action.preventDefault(); open(event); } }}>{event.title}</a></h3>
                <p>{eventWhen(event)}</p>{event.location && <p>{event.location}</p>}<p>{responseLabels[event.my_rsvp]} · #arr:{event.code}</p>
                {event.needs_attention && <p className="sp-event-notice">Arrangementet er endra sidan du svarte.</p>}
              </li>)}</ul></section> : null;
            })}
            {nextAfter && <Button disabled={loadingMore} onClick={() => setAfter(nextAfter)}>Last fleire arrangement</Button>}
          </>}
        </div>
      </section>
    </div>
  </EventReferencesContext.Provider>;
}

export function EventPills({ events, selected, onSelect, more: hasMore, loading, onLoadMore }: { events: readonly ChannelEvent[]; selected: string | null; onSelect(id: string | null): void; more?: boolean; loading?: boolean; onLoadMore?(): void }) {
  const [expanded, setExpanded] = useState(false);
  if (!events.length) return null;
  const ordered = [...events].sort((a, b) => a.id.localeCompare(b.id));
  const visible = expanded ? ordered : ordered.slice(0, 3);
  const active = ordered.find(event => event.id === selected);
  if (active && !visible.some(event => event.id === active.id)) visible.push(active);
  const more = ordered.filter(event => !visible.some(item => item.id === event.id));
  return <>
    {visible.map(event => <Button className="sp-event-pill" variant="quiet" key={event.id} aria-pressed={selected === event.id}
      aria-label={`${event.title}, ${eventWhen(event)}`} title={`${event.title}
${eventWhen(event)}
#arr:${event.code}`} onClick={() => onSelect(event.id)}><span>{event.title}</span></Button>)}
    {ordered.length > 3 && <Button variant="quiet" aria-expanded={expanded} onClick={() => setExpanded(value => !value)}>{expanded ? "Færre arrangement" : `Fleire arrangement (${more.length})`}</Button>}
    {hasMore && <Button variant="quiet" disabled={loading} onClick={onLoadMore}>Last fleire arrangement</Button>}
  </>;
}

function EventMessageResults({ api, owner, channelId, eventId, timeline, onDenied }: Scope & { eventId: string; timeline: TimelineProps; onDenied(): void }) {
  const [page, setPage] = useState<EventMessages>({ messages: [], next_before: null });
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [request, setRequest] = useState({ before: null as number | null, attempt: 0 });
  const pagesLoaded = useRef(1);
  const liveRevision = JSON.stringify(timeline.messages.map(message => [message.id, message.body, message.edited_at, message.deleted_at]));
  const previousLiveRevision = useRef(liveRevision);
  useEffect(() => {
    const controller = new AbortController(); setLoading(true); setError("");
    const before = previousLiveRevision.current === liveRevision ? request.before : null;
    previousLiveRevision.current = liveRevision;
    async function load() {
      let value = await api.messages(owner, channelId, eventId, before, controller.signal);
      if (before === null) {
        for (let page = 1; page < pagesLoaded.current && value.next_before !== null; page++) {
          const next = await api.messages(owner, channelId, eventId, value.next_before, controller.signal);
          value = { ...next, messages: [...value.messages, ...next.messages] };
        }
      } else if (!controller.signal.aborted) pagesLoaded.current++;
      return value;
    }
    void load().then(value => {
      if (!controller.signal.aborted) { setPage(previous => ({ ...value, messages: before === null ? value.messages : [...previous.messages, ...value.messages] })); setLoading(false); }
    }, cause => { if (!controller.signal.aborted) { setLoading(false); setError("Arrangementstreffa kunne ikkje lastast."); if (denied(cause)) { setPage({ messages: [], next_before: null }); onDenied(); } } });
    return () => controller.abort();
  }, [api, owner, channelId, eventId, request, liveRevision]);
  const latest = new Map(timeline.messages.map(message => [message.id, message]));
  const messages = [...new Map(page.messages.map(message => [message.id, latest.get(message.id) ?? message])).values()].sort((a, b) => b.sequence - a.sequence);
  return <div className="sp-event-results"><ConversationTimeline {...timeline} messages={messages}
    items={undefined} notices={undefined} viewportRef={undefined} onScroll={undefined} unreadAfterSequence={undefined}
    loading={loading} error={error || undefined} hasOlder={page.next_before !== null}
    onRetry={() => setRequest(value => ({ ...value, attempt: value.attempt + 1 }))}
    onLoadOlder={() => setRequest(value => ({ before: page.next_before, attempt: value.attempt + 1 }))}
    matches={{ label: "Meldingar om arrangementet", onOpen(message) {
      const url = new URL(window.location.href); url.searchParams.delete("event"); url.searchParams.set("channel", channelId);
      url.searchParams.set("message", message.id); url.searchParams.set("sequence", String(message.sequence));
      if (message.parent_message_id) url.searchParams.set("thread", message.parent_message_id);
      window.location.assign(url);
    } }} /></div>;
}

function EventDetail({ api, owner, channelId, eventId, refresh, onUpdate, onEdit, onNew, onConversation, onBack, onDenied, uploadImage, readOnly = false }: Scope & {
  readOnly?: boolean;
  eventId: string; refresh: number; onUpdate(event: ChannelEvent): void; onEdit(event: ChannelEvent): void; onNew(): void; onConversation(event: ChannelEvent, compose: boolean): void; onBack(): void; onDenied(): void;
  uploadImage(file: File, progress: (text: string) => void): Promise<string>;
}) {
  const [event, setEvent] = useState<ChannelEvent>();
  const [people, setPeople] = useState<readonly EventPerson[]>([]);
  const [peopleNext, setPeopleNext] = useState<string | null>(null);
  const [peopleCounts, setPeopleCounts] = useState<Record<EventResponse, number> | null>(null);
  const [error, setError] = useState("");
  const [peopleError, setPeopleError] = useState(false);
  const [busy, setBusy] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const [status, setStatus] = useState("");
  const [copied, setCopied] = useState(false);
  const [lightbox, setLightbox] = useState<string>();
  const [confirmCancel, setConfirmCancel] = useState(false);
  const [featureDisabled, setFeatureDisabled] = useState(false);
  const alive = useRef(true);
  const imageRequest = useRef<AbortController | null>(null);
  const peopleRequest = useRef<AbortController | null>(null);
  const fileInput = useRef<HTMLInputElement | null>(null);
  const heading = useRef<HTMLHeadingElement | null>(null);
  useEffect(() => { if (event) heading.current?.focus({ preventScroll: true }); }, [Boolean(event)]);
  useEffect(() => { alive.current = true; return () => { alive.current = false; imageRequest.current?.abort(); peopleRequest.current?.abort(); }; }, []);
  useEffect(() => {
    const controller = new AbortController();
    void api.detail(owner, channelId, eventId, controller.signal).then(value => {
      if (controller.signal.aborted) return;
      setEvent(value); setError(""); onUpdate(value);
      if (value.events_enabled === true) setFeatureDisabled(false);
    }, cause => { if (!controller.signal.aborted) { setError(`Arrangementet kunne ikkje lastast. ${errorText(cause)}`); if (denied(cause)) { setEvent(undefined); setPeople([]); setLightbox(undefined); onDenied(); } } });
    return () => controller.abort();
  }, [api, owner, channelId, eventId, refresh, attempt, readOnly]);
  useEffect(() => {
    const controller = new AbortController(); setPeopleError(false);
    peopleRequest.current?.abort(); peopleRequest.current = controller;
    void api.responses(owner, channelId, eventId, controller.signal).then(value => { if (!controller.signal.aborted) { setPeople(value.responses); setPeopleNext(value.next_after); setPeopleCounts(value.counts); } }, cause => {
      if (!controller.signal.aborted) { setPeopleError(true); if (denied(cause)) { setPeople([]); setEvent(undefined); onDenied(); } }
    });
    return () => controller.abort();
  }, [api, owner, channelId, eventId, event?.revision, event?.my_rsvp, attempt, refresh]);
  async function mutate(action: () => Promise<ChannelEvent>) {
    if (readOnly || featureDisabled || event?.events_enabled !== true) return;
    setBusy(true); setError("");
    try { const value = await action(); if (alive.current) { setEvent(value); onUpdate(value); setAttempt(value => value + 1); } }
    catch (cause) { if (alive.current) { setError(`Endringa kunne ikkje lagrast. ${errorText(cause)}`); if (cause instanceof HttpError && cause.status === 409 && /slått av|disabled/iu.test(cause.message)) setFeatureDisabled(true); if (denied(cause)) { setEvent(undefined); setPeople([]); onDenied(); } } }
    finally { if (alive.current) { setBusy(false); setStatus(""); } }
  }
  const archived = readOnly || featureDisabled || event?.events_enabled === false;
  const canWrite = !readOnly && !featureDisabled && event?.events_enabled === true;
  return <article className="sp-event-detail">
    <Button variant="quiet" onClick={onBack}>{readOnly ? "← Tilbake til meldingane" : "← Alle arrangement"}</Button>
    {canWrite && <Button variant="primary" onClick={onNew}>Nytt arrangement</Button>}
    {archived && <Status>Arrangement er slått av i denne kanalen. Du kan lese arkivet, men ikkje endre arrangement eller svar.</Status>}
    {error && <Status tone="error">{error} <Button onClick={() => setAttempt(value => value + 1)}>Prøv igjen</Button></Status>}
    {!event && !error && <Status>Lastar arrangement …</Status>}
    {event && <>
      <h2 ref={heading} tabIndex={-1}>{event.title}</h2>
      {event.cancelled && <Status>Arrangementet er avlyst.</Status>}
      {event.needs_attention && <div className="sp-event-notice" role="status"><p>Arrangementet er endra sidan du svarte. Sjå gjennom tid, stad og omtale.</p>{canWrite && <Button disabled={busy} onClick={() => void mutate(() => api.change(owner, channelId, event.id, "acknowledge"))}>Markér som lesen</Button>}</div>}
      <p>{eventWhen(event)}</p>{event.location && <p>{event.location}</p>}
      <fieldset className="sp-event-response" disabled={busy || event.cancelled || !canWrite}><legend>Ditt svar</legend>{eventResponses.map(response => <label key={response}>
        <input type="radio" name={`rsvp-${event.id}`} value={response} checked={event.my_rsvp === response} onChange={() => void mutate(() => api.change(owner, channelId, event.id, "rsvp", { status: response }))} />{responseLabels[response]}</label>)}</fieldset>
      {!archived && <div className="sp-event-actions"><Button onClick={() => onConversation(event, false)}>Vis samtalen</Button><Button onClick={() => onConversation(event, true)}>Skriv om arrangementet</Button></div>}
      <p>Bruk <code>#arr:{event.code}</code> for å merke ei melding med dette arrangementet.</p>
      {event.cover_media_id && <PreviewImage className="sp-event-cover" id={event.cover_media_id} name={`Forside for ${event.title}`} />}
      <MarkdownContent source={event.description} />
      <div className="sp-event-actions"><Button onClick={() => { void navigator.clipboard.writeText(`#arr:${event.code}`).then(() => setCopied(true), () => setError("Kopiering feila. Marker og kopier koden ovanfor.")); }}>{copied ? "Kopiert" : "Kopier #arr-kode"}</Button>
        <a href={eventHref(channelId, event.id)}>Detaljlenkje</a>
        {canWrite && event.can_manage && <><Button disabled={busy} onClick={() => onEdit(event)}>Rediger</Button>{!event.cancelled && <Button disabled={busy} onClick={() => setConfirmCancel(true)}>Avlys arrangement</Button>}</>}
      </div>
      {confirmCancel && canWrite && <div className="sp-event-notice"><p>Avlyse {event.title}? Arrangementet og tidlegare svar blir bevarte.</p><Button variant="danger" disabled={busy} onClick={() => { setConfirmCancel(false); void mutate(() => api.change(owner, channelId, event.id, "cancel")); }}>Stadfest avlysing</Button><Button onClick={() => setConfirmCancel(false)}>Behald arrangementet</Button></div>}
      <section aria-label="Deltakarsvar"><h3>Deltakarsvar</h3>{peopleError ? <Status tone="error">Deltakarsvara kunne ikkje lastast. <Button onClick={() => setAttempt(value => value + 1)}>Prøv igjen</Button></Status> : eventResponses.map(response => {
        const respondents = people.filter(person => person.status === response);
        return <details key={response}><summary>{responseLabels[response]} ({peopleCounts?.[response] ?? respondents.length}{!peopleCounts && peopleNext ? "+" : ""})</summary>{respondents.length ? <ul>{respondents.map(person => <li key={person.user_id}>{person.display_name}</li>)}</ul> : <p>{peopleNext ? "Last fleire for å sjå fleire namn." : "Ingen registrerte svar."}</p>}</details>;
      })}{peopleNext && <Button disabled={busy} onClick={() => {
        peopleRequest.current?.abort(); const controller = new AbortController(); peopleRequest.current = controller; setBusy(true);
        void api.responses(owner, channelId, eventId, controller.signal, peopleNext).then(page => {
          if (!controller.signal.aborted) { setPeople(previous => [...new Map([...previous, ...page.responses].map(person => [person.user_id, person])).values()]); setPeopleNext(page.next_after); setPeopleCounts(page.counts); }
        }, cause => { if (!controller.signal.aborted) { setPeopleError(true); if (denied(cause)) { setEvent(undefined); setPeople([]); onDenied(); } } })
          .finally(() => { if (!controller.signal.aborted) setBusy(false); });
      }}>Vis fleire deltakarar</Button>}</section>
      <section aria-label="Biletbibliotek"><h3>Biletbibliotek</h3><p>Alle i kanalen kan bidra. Du kan fjerne eigne bilete; arrangementsansvarlege kan administrere alle.</p>
        {canWrite && <Button disabled={busy} onClick={() => fileInput.current?.click()}>Legg til bilete</Button>}
        <input ref={fileInput} aria-label="Legg til bilete" hidden type="file" accept="image/*" multiple disabled={busy || !canWrite} onChange={action => {
          const files = [...(action.currentTarget.files ?? [])]; action.currentTarget.value = "";
          if (!files.length) return;
          void mutate(async () => {
            let updated = event;
            for (const file of files) {
              if (!file.type.startsWith("image/")) throw new Error("Vel ei biletefil.");
              const mediaId = await uploadImage(file, text => { if (alive.current) setStatus(text); });
              if (!alive.current) throw new Error("Kanalen vart endra.");
              updated = await api.change(owner, channelId, event.id, "images", { media_id: mediaId });
            }
            return updated;
          });
        }} />
        {status && <Status>{status}</Status>}
        {!event.images.length && <p>Ingen bilete enno.</p>}
        <div className="sp-event-gallery">{event.images.map(image => <figure key={image.media_id}><Button variant="quiet" aria-label="Opne bilete" onClick={() => setLightbox(image.media_id)}><PreviewImage id={image.media_id} name="Bilete frå arrangementet" /></Button><figcaption>
          {event.cover_media_id === image.media_id && <span>Forside</span>}
          {canWrite && event.can_manage && <Button variant="quiet" disabled={busy} onClick={() => void mutate(() => api.change(owner, channelId, event.id, "cover", { media_id: event.cover_media_id === image.media_id ? null : image.media_id }))}>{event.cover_media_id === image.media_id ? "Fjern forside" : "Bruk som forside"}</Button>}
          {canWrite && (event.can_manage || image.contributor_id === owner) && <Button variant="quiet" disabled={busy} onClick={() => void mutate(() => api.removeImage(owner, channelId, event.id, image.media_id))}>Fjern bilete</Button>}
        </figcaption></figure>)}</div>
        {event.images_next_after && <Button disabled={busy} onClick={() => {
          imageRequest.current?.abort(); const controller = new AbortController(); imageRequest.current = controller; setBusy(true); setError("");
          void api.images(owner, channelId, event.id, event.images_next_after!, controller.signal).then(page => {
            if (!controller.signal.aborted) setEvent(previous => previous ? { ...previous, images: [...previous.images, ...page.images], images_next_after: page.next_after } : previous);
          }, cause => { if (!controller.signal.aborted) { setError(errorText(cause)); if (denied(cause)) { setEvent(undefined); onDenied(); } } })
            .finally(() => { if (!controller.signal.aborted) setBusy(false); });
        }}>Fleire bilete</Button>}
      </section>
      {lightbox && <ImageViewer src={mediaUrl(lightbox, true)} downloadSrc={mediaUrl(lightbox, true)} name={event.title} onClose={() => setLightbox(undefined)} />}
    </>}
  </article>;
}

function localInput(instant: string, timezone: string) {
  try {
    const parts = new Intl.DateTimeFormat("sv-SE", { timeZone: timezone, year: "numeric", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit", hourCycle: "h23" }).formatToParts(new Date(instant));
    const part = (type: Intl.DateTimeFormatPartTypes) => parts.find(part => part.type === type)?.value ?? "";
    return `${part("year")}-${part("month")}-${part("day")}T${part("hour")}:${part("minute")}`;
  } catch { return ""; }
}
/** Resolve wall time in the selected zone, rejecting DST gaps instead of silently moving a meeting. */
export function eventInstant(local: string, timezone: string): string {
  if (!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}$/u.test(local)) throw new Error("Vel ein gyldig dato og eit klokkeslett.");
  const desired = Date.parse(`${local}Z`);
  let candidate = desired;
  for (let attempt = 0; attempt < 4; attempt++) {
    const formatted = localInput(new Date(candidate).toISOString(), timezone);
    if (!formatted) throw new Error("Vel ei gyldig IANA-tidssone, til dømes Europe/Oslo.");
    if (formatted === local) return new Date(candidate).toISOString();
    candidate += desired - Date.parse(`${formatted}Z`);
  }
  throw new Error("Klokkeslettet finst ikkje i denne tidssona på grunn av sommartid. Vel eit anna klokkeslett.");
}
function EventEditor({ event, onSave, onClose }: { event?: ChannelEvent; onSave(input: EventInput, requestId?: string): Promise<void>; onClose(): void }) {
  const timezone = event?.timezone ?? Intl.DateTimeFormat().resolvedOptions().timeZone ?? "Europe/Oslo";
  const [draft, setDraft] = useState({ title: event?.title ?? "", code: event?.code ?? "", timezone, location: event?.location ?? "", description: event?.description ?? "",
    start: event ? localInput(event.starts_at, timezone) : "", end: event?.ends_at ? localInput(event.ends_at, timezone) : "" });
  const [error, setError] = useState(""); const [busy, setBusy] = useState(false); const id = useId();
  const [requestId] = useState(() => event ? undefined : crypto.randomUUID());
  const [uncertain, setUncertain] = useState(false);
  const attemptedInput = useRef<EventInput | null>(null);
  const locked = busy || uncertain;
  const field = (key: keyof typeof draft, value: string) => setDraft(previous => ({ ...previous, [key]: value }));
  return <form className="sp-event-form" onSubmit={action => {
    action.preventDefault(); if (busy) return; setError("");
    let input: EventInput;
    try {
      const start = eventInstant(draft.start, draft.timezone); const end = draft.end ? eventInstant(draft.end, draft.timezone) : null;
      if (end && end <= start) throw new Error("Sluttida må vere etter starttida.");
      input = { title: draft.title.trim(), code: draft.code.trim().toLowerCase(), timezone: draft.timezone.trim(), starts_at: start, ends_at: end, location: draft.location.trim() || null, description: draft.description };
    } catch (cause) { setError(errorText(cause)); return; }
    if (uncertain && attemptedInput.current) input = attemptedInput.current;
    attemptedInput.current = input;
    setBusy(true); void onSave(input, requestId).catch(cause => {
      const unresolved = !event && (!(cause instanceof HttpError) || cause.status >= 500);
      setUncertain(unresolved); setError(`${errorText(cause)}${unresolved ? " Kvitteringa er uviss. Prøv å lagre det same utkastet igjen før du lagar eit nytt arrangement." : ""}`); setBusy(false);
    });
  }}>
    <h2 tabIndex={-1}>{event ? "Rediger arrangement" : "Nytt arrangement"}</h2>
    {!event && <p>Lag eit nytt arrangement i denne kanalen. Arrangement med same tittel får kvar sin kode.</p>}
    {event && <p>Vesentlege endringar får ein ny versjon synleg for dei som deltek eller følgjer arrangementet.</p>}
    <label htmlFor={`${id}-title`}>Tittel<input autoFocus id={`${id}-title`} required maxLength={160} value={draft.title} disabled={locked} onChange={action => field("title", action.currentTarget.value)} /></label>
    <details className="sp-event-options"><summary>Fleire val</summary>
      <label htmlFor={`${id}-code`}>Kode i kanalen<input id={`${id}-code`} aria-label="Kode i kanalen" required={Boolean(event)} pattern="[a-z0-9-]{1,48}" maxLength={48} placeholder="Laga automatisk" value={draft.code} disabled={locked} onChange={action => field("code", action.currentTarget.value.toLowerCase())} aria-describedby={`${id}-code-help`} /><span id={`${id}-code-help`}>Unik kode med små bokstavar, tal og bindestrek. {event ? "Eksisterande kode blir bevart når du endrar tittelen." : "La feltet stå tomt for å lage ein kode automatisk."}</span></label>
    </details>
    <label htmlFor={`${id}-zone`}>Tidssone<input id={`${id}-zone`} required value={draft.timezone} disabled={locked} onChange={action => field("timezone", action.currentTarget.value)} placeholder="Europe/Oslo" /></label>
    <label htmlFor={`${id}-start`}>Start<input id={`${id}-start`} type="datetime-local" required value={draft.start} disabled={locked} onChange={action => field("start", action.currentTarget.value)} /></label>
    <label htmlFor={`${id}-end`}>Slutt (valfritt)<input id={`${id}-end`} type="datetime-local" value={draft.end} disabled={locked} onChange={action => field("end", action.currentTarget.value)} /></label>
    <label htmlFor={`${id}-place`}>Stad (valfritt)<input id={`${id}-place`} maxLength={300} value={draft.location} disabled={locked} onChange={action => field("location", action.currentTarget.value)} /></label>
    <label htmlFor={`${id}-description`}>Omtale (Markdown)<textarea id={`${id}-description`} maxLength={20000} value={draft.description} disabled={locked} onChange={action => field("description", action.currentTarget.value)} /></label>
    {draft.description && <details><summary>Førehandsvis omtale</summary><MarkdownContent source={draft.description} /></details>}
    {error && <Status tone="error">{error}</Status>}
    <div className="sp-event-actions"><Button type="submit" variant="primary" busy={busy}>Lagre arrangement</Button><Button disabled={busy} onClick={onClose}>Avbryt</Button></div>
  </form>;
}
