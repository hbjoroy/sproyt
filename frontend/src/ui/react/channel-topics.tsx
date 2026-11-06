import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { Button, Status } from "@sproyt/ui/react";
import { HttpError } from "../../api";
import type { ChannelTopic, ChannelTopicMatch, ChannelTopicPage, ChannelTopicSummary, ChannelTopicsApi } from "../../channel-topics";
import type { ChatMessage } from "../../types";
import { ConversationTimeline, formatMessageDateTime, type TimelineProps } from "./conversation-view";
import { markdownTextFromMessage } from "./markdown-content";
import { personalMessageReference } from "../../personal-space";

type TopicScope = Readonly<{ api: ChannelTopicsApi; owner: string; channelId: string }>;

function useSummary({ api, owner, channelId }: TopicScope, refresh = false) {
  const [summary, setSummary] = useState<ChannelTopicSummary>();
  const [error, setError] = useState(false);
  const [revision, setRevision] = useState(0);
  const reload = () => setRevision(value => value + 1);
  useEffect(() => api.subscribe(channelId, reload), [api, channelId]);
  useEffect(() => {
    if (!refresh) return;
    const visible = () => { if (document.visibilityState === "visible") reload(); };
    const timer = window.setInterval(visible, 30_000);
    document.addEventListener("visibilitychange", visible);
    return () => {
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", visible);
    };
  }, [refresh]);
  useEffect(() => {
    const controller = new AbortController();
    setError(false);
    void api.summary(owner, channelId, controller.signal).then(value => {
      if (!controller.signal.aborted) setSummary(value);
    }, cause => {
      if (controller.signal.aborted) return;
      if (cause instanceof HttpError && [401, 403, 404].includes(cause.status)) setSummary(undefined);
      setError(true);
    });
    return () => controller.abort();
  }, [api, owner, channelId, revision]);
  return { summary, error, reload, setSummary };
}

export function ChannelTopicSetting({ api, owner, channelId }: TopicScope) {
  const { summary, error, reload, setSummary } = useSummary({ api, owner, channelId });
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState(false);
  if (!summary?.can_manage && !error) return null;
  return <section className="sp-settings-group" aria-label="Automatiske emne">
    <h3>Automatiske emne</h3>
    {summary?.can_manage && <>
      <p>Vis emne som går att i meldingane i denne samtalen.</p>
      <label className="sp-topic-setting"><input type="checkbox" role="switch" checked={summary.enabled} disabled={saving} onChange={event => {
        const next = event.currentTarget.checked;
        setSaving(true);
        setSaveError(false);
        void api.save(owner, channelId, next).then(setSummary, () => setSaveError(true)).finally(() => setSaving(false));
      }} /> Vis automatiske emne</label>
    </>}
    {error && <Status tone="error">Emneinnstillinga kunne ikkje lastast. <Button onClick={reload}>Prøv igjen</Button></Status>}
    {saveError && <Status tone="error">Emneinnstillinga kunne ikkje lagrast. Prøv igjen.</Status>}
  </section>;
}

type Results = ChannelTopicPage & { readonly loading: boolean; readonly error: boolean };
const emptyResults: Results = { matches: [], next_before: null, loading: false, error: false };

function resultText(body: string) {
  if (personalMessageReference(body, window.location.origin)) return "Lagra melding";
  return markdownTextFromMessage(body).replace(/!?\[([^\]\n]*)\]\([^\s)]+\)/gu, "$1")
    .replace(/^\s*(?:#{1,6}|>|[-*])\s+/gmu, "").trim() || "Melding med vedlegg";
}

export function ChannelTopics({ api, owner, channelId, timeline, resetRequest, active, onThread }: TopicScope & {
  readonly timeline: TimelineProps;
  readonly resetRequest: number;
  readonly active: boolean;
  readonly onThread: (match: ChannelTopicMatch) => void;
}) {
  const { summary } = useSummary({ api, owner, channelId }, active);
  const [selection, setSelection] = useState<{ topic: ChannelTopic; resetRequest: number }>();
  const selected = summary?.enabled && selection?.resetRequest === resetRequest ? selection.topic : undefined;
  const [results, setResults] = useState<Results>(emptyResults);
  const request = useRef<AbortController>(null);
  const [context, setContext] = useState<ChannelTopicMatch>();
  const [contextMessages, setContextMessages] = useState<readonly ChatMessage[]>();
  const [contextError, setContextError] = useState(false);
  const [contextAttempt, setContextAttempt] = useState(0);
  const contextViewport = useRef<HTMLDivElement>(null);
  const activeResults = useRef<HTMLDivElement>(null);
  const openedFrom = useRef<HTMLElement>(null);
  const resultId = useId();
  const selectedKey = selected?.key;

  function loadPage(topic: string, before: number | null) {
    request.current?.abort();
    const controller = new AbortController();
    request.current = controller;
    setResults(previous => ({ ...(before === null ? emptyResults : previous), loading: true, error: false }));
    void api.matches(owner, channelId, topic, before, controller.signal).then(page => {
      if (!controller.signal.aborted) setResults(previous => ({
        ...page, matches: before === null ? page.matches : [...previous.matches, ...page.matches], loading: false, error: false
      }));
    }, cause => {
      if (!controller.signal.aborted) setResults(previous => ({
        ...(cause instanceof HttpError && [401, 403, 404].includes(cause.status) ? emptyResults : previous), loading: false, error: true
      }));
    });
  }

  useEffect(() => {
    setContext(undefined);
    if (selectedKey) {
      loadPage(selectedKey, null);
      if (activeResults.current) activeResults.current.scrollTop = 0;
    } else setSelection(undefined);
    return () => request.current?.abort();
  }, [api, owner, channelId, selectedKey]);

  const activeContext = selectedKey ? context : undefined;
  useEffect(() => {
    if (!activeContext) return;
    const controller = new AbortController();
    setContextMessages(undefined);
    setContextError(false);
    void api.context(owner, channelId, activeContext, controller.signal).then(messages => {
      if (!controller.signal.aborted) setContextMessages(messages);
    }, () => {
      if (!controller.signal.aborted) setContextError(true);
    });
    return () => controller.abort();
  }, [api, owner, channelId, activeContext, contextAttempt]);

  useLayoutEffect(() => {
    if (!activeContext || !contextMessages) return;
    const row = contextViewport.current?.querySelector<HTMLElement>(`[data-message-id="${CSS.escape(activeContext.message_id)}"]`);
    if (!row) return;
    row.tabIndex = -1;
    row.scrollIntoView({ block: "center" });
    row.focus({ preventScroll: true });
  }, [activeContext, contextMessages]);

  const available = summary?.enabled ? summary.topics : [];
  const topics = selected && !available.some(topic => topic.key === selected.key) ? [...available.slice(0, 5), selected] : available;
  const hidden = Boolean(selectedKey);
  const latestMessages = activeContext && contextMessages ? new Map(timeline.messages.map(message => [message.id, message])) : null;
  const contextRows = contextMessages?.map(message => latestMessages?.get(message.id) ?? message);
  const targetRemoved = activeContext && latestMessages?.get(activeContext.message_id)?.deleted_at;
  return <>
    {topics.length > 0 && <div className="sp-topic-pills" role="group" aria-label="Emne i samtalen">
      <Button variant="quiet" aria-pressed={!selected} aria-controls={resultId} onClick={() => setSelection(undefined)}>Alle</Button>
      {topics.map(topic => <Button variant="quiet" key={topic.key} aria-pressed={selectedKey === topic.key}
        aria-controls={resultId} onClick={() => setSelection({ topic, resetRequest })}>{topic.label}</Button>)}
    </div>}
    <ConversationTimeline {...timeline} id={hidden ? undefined : resultId} hidden={hidden} />
    {selected && <div className="sp-topic-viewport" id={resultId}>
        <div className="sp-topic-results" ref={activeResults} hidden={Boolean(activeContext)}
          aria-label={`Meldingar om ${selected.label}`} aria-busy={results.loading}>
          {results.matches.length > 0 && <ul>{results.matches.map(match => <li key={match.message_id}>
            <Button variant="quiet" onClick={event => {
              if (match.parent_message_id) onThread(match);
              else {
                openedFrom.current = event.currentTarget;
                setContextMessages(undefined);
                setContextError(false);
                setContext(match);
              }
            }}>
              <span className="sp-topic-result-meta"><strong>{match.sender_display_name}</strong>
                <time dateTime={match.created_at}>{formatMessageDateTime(match.created_at)}</time></span>
              <span className="sp-topic-result-body">{resultText(match.body)}</span>
            </Button>
          </li>)}</ul>}
          {results.loading && <Status>Lastar emnetreff …</Status>}
          {!results.loading && !results.error && results.matches.length === 0 && <Status>Ingen meldingar med dette emnet.</Status>}
          {results.error && <Status tone="error">Emnetreffa kunne ikkje lastast. <Button onClick={() => loadPage(selected.key, results.next_before)}>Prøv igjen</Button></Status>}
          {results.next_before !== null && !results.error && <Button busy={results.loading}
            onClick={() => loadPage(selected.key, results.next_before)}>Last eldre treff</Button>}
        </div>
        {activeContext && <section className="sp-topic-context" aria-label="Meldinga i samtalen">
          <div className="sp-topic-context-header"><Button variant="quiet" onClick={() => {
            setContext(undefined);
            requestAnimationFrame(() => openedFrom.current?.focus());
          }}>Tilbake til emnetreffa</Button></div>
          {targetRemoved && <Status>Meldinga er sletta.</Status>}
          {!targetRemoved && contextError && <Status tone="error">Meldinga eller samanhengen kunne ikkje lastast. <Button onClick={() => setContextAttempt(value => value + 1)}>Prøv igjen</Button></Status>}
          {!contextRows && !contextError && <Status>Lastar meldinga i samtalen …</Status>}
          {!targetRemoved && contextRows && <ConversationTimeline {...timeline} messages={contextRows} items={undefined}
            viewportRef={contextViewport} onScroll={undefined} loading={false} error={undefined} hasOlder={false}
            onLoadOlder={undefined} unreadAfterSequence={undefined} notices={undefined} />}
        </section>}
    </div>}
  </>;
}
