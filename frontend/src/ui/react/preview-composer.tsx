import { Button, Dialog, Status } from "@sproyt/ui/react";
import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import type { ComposerSnapshot } from "../../application/composer-controller";
import type { ComposerTarget } from "./host-adapter";
import { openEmojiPicker } from "./emoji-picker";
import { pastedEmoji, type SavedEmojiApi } from "../../saved-emojis";
import { DraftComposer } from "./draft-composer";
import { PreviewAttachments } from "./preview-media";
import type { AgentLocationApi } from "../../agent-locations";
import { AgentLocationSharingDialog } from "./agent-location-sharing";
import { SPROYT_EVENT_COMPOSE_EVENT, useEventReferences } from "./event-references";
import type { ChannelEvent } from "../../channel-events";
import { eventContextWhen, messageWithEventContext } from "./event-composer-context";
import eventContextCss from "./event-composer-context.css";

export interface PreviewMention {
  readonly id: string;
  readonly name: string;
  readonly handle: string;
  readonly expandsDirect: boolean;
  readonly eventCode?: string;
}

export type PreviewComposerState = ComposerSnapshot;

export interface PreviewComposerHost {
  readonly savedEmojis: SavedEmojiApi;
  readonly composer: (target: ComposerTarget) => PreviewComposerState;
  readonly changeDraft: (target: ComposerTarget, value: string) => void;
  readonly send: (target: ComposerTarget) => void;
  readonly upload: (target: ComposerTarget, files: readonly File[]) => void;
  readonly removeMedia: (target: ComposerTarget, id: string) => void;
  readonly mentionCandidates: (target: ComposerTarget) => readonly PreviewMention[];
  readonly expandDirect: (target: ComposerTarget, userId: string) => void;
  readonly openImageGeneration: () => void;
  readonly agentLocations: AgentLocationApi;
}

/** Host state owns drafts and delivery; selection and transient suggestions
 * belong to the visible field. Hidden legacy fields never receive key events. */
export function PreviewComposer({ host, target }: {
  host: PreviewComposerHost; target: ComposerTarget;
}) {
  const eventReferences = useEventReferences();
  const composing = useRef(false);
  const container = useRef<HTMLDivElement>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const restoreFocus = useRef(false);
  const closeEmoji = useRef<(() => void) | undefined>(undefined);
  const selection = useRef({ start: 0, end: 0 });
  const pendingSelection = useRef<number | null>(null);
  const [emojiSaveError, setEmojiSaveError] = useState("");
  const [caret, setCaret] = useState<number | null>(null);
  const [dismissed, setDismissed] = useState(false);
  const [selected, setSelected] = useState(0);
  const [expansion, setExpansion] = useState<PreviewMention | null>(null);
  const [locationOpen, setLocationOpen] = useState(false);
  const [eventContext, setEventContext] = useState<ChannelEvent | null>(null);
  const [eventSendError, setEventSendError] = useState("");
  const [eventPickerOpen, setEventPickerOpen] = useState(false);
  const [eventSearch, setEventSearch] = useState("");
  const [pickerResults, setPickerResults] = useState<readonly ChannelEvent[]>([]);
  const [pickerError, setPickerError] = useState("");
  const [pickerLoading, setPickerLoading] = useState(false);
  const [pickerAttempt, setPickerAttempt] = useState(0);
  const preparedEventDraft = useRef<{ before: string; after: string } | null>(null);
  const listId = useId();
  const state = host.composer(target);
  const input = () => container.current?.querySelector("textarea");
  const eventContextAvailable = !target.parentMessageId && Boolean(eventReferences?.suggest);
  const activeEventContext = eventContextAvailable && eventContext?.channel_id === target.channelId
    ? eventReferences?.events.find(event => event.id === eventContext.id) ?? eventContext : null;
  const pickerEvents = [...new Map([...(eventReferences?.events ?? []), ...pickerResults].map(event => [event.id, event])).values()]
    .filter(event => !event.cancelled && event.channel_id === target.channelId && (!eventSearch || `${event.title} ${event.code} ${event.aliases.join(" ")}`.toLocaleLowerCase().includes(eventSearch.toLocaleLowerCase())))
    .sort((left, right) => left.title.localeCompare(right.title));
  const chooseEventContext = (event: ChannelEvent | null) => {
    const draft = host.composer(target);
    if (draft.disabled || draft.busy) return;
    // An unsuccessfully sent suffix is ours only while the draft is unchanged.
    // Text the user has edited, including manual references, remains theirs.
    if (preparedEventDraft.current && draft.value === preparedEventDraft.current.after) host.changeDraft(target, preparedEventDraft.current.before);
    preparedEventDraft.current = null;
    setEventContext(event); setEventPickerOpen(false); setEventSendError("");
    queueMicrotask(() => input()?.focus({ preventScroll: true }));
  };
  useEffect(() => {
    if (!eventContextAvailable) { setEventContext(null); setEventPickerOpen(false); setEventSendError(""); preparedEventDraft.current = null; }
  }, [eventContextAvailable, target.channelId]);
  useEffect(() => { setEventContext(null); setEventPickerOpen(false); setEventSendError(""); preparedEventDraft.current = null; }, [target.channelId]);
  useEffect(() => {
    if (!eventContextAvailable) return;
    const compose = (action: Event) => {
      const event = (action as CustomEvent<ChannelEvent>).detail;
      if (event?.channel_id === target.channelId) chooseEventContext(event);
    };
    window.addEventListener(SPROYT_EVENT_COMPOSE_EVENT, compose);
    return () => window.removeEventListener(SPROYT_EVENT_COMPOSE_EVENT, compose);
  }, [eventContextAvailable, target.channelId, state.disabled, state.busy, host]);
  useEffect(() => {
    if (!eventPickerOpen || !eventReferences?.search) return;
    const controller = new AbortController();
    setPickerLoading(true); setPickerError(""); setPickerResults([]);
    const timer = window.setTimeout(() => {
      void eventReferences.search!(eventSearch.trim(), controller.signal).then(events => {
        if (!controller.signal.aborted) { setPickerResults(events); setPickerLoading(false); }
      }, () => { if (!controller.signal.aborted) { setPickerError("Arrangementa kunne ikkje lastast."); setPickerLoading(false); } });
    }, 150);
    return () => { clearTimeout(timer); controller.abort(); };
  }, [eventPickerOpen, eventSearch, eventReferences?.search, pickerAttempt]);
  const updateSelection = (element: HTMLTextAreaElement) => {
    selection.current = { start: element.selectionStart, end: element.selectionEnd };
    setCaret(element.selectionStart === element.selectionEnd ? element.selectionStart : null);
  };
  const match = !dismissed && !composing.current && caret !== null
    ? state.value.slice(0, caret).match(/(?:^|\s)@([\p{L}\p{N}_.-]*)$/u) : null;
  const query = match?.[1]?.toLocaleLowerCase();
  const eventMatch = eventReferences?.suggest && !dismissed && !composing.current && caret !== null
    ? state.value.slice(0, caret).match(/(?:^|\s)#arr:([a-z0-9-]*)$/iu) : null;
  const eventQuery = eventMatch?.[1]?.toLowerCase();
  const [eventSuggestions, setEventSuggestions] = useState<{ query: string; events: readonly ChannelEvent[] }>();
  const [eventSuggestionError, setEventSuggestionError] = useState(false);
  useEffect(() => {
    if (!eventQuery || eventQuery.length > 48 || !eventReferences?.suggest || state.disabled || state.busy) return;
    const controller = new AbortController(); setEventSuggestionError(false);
    const timer = window.setTimeout(() => {
      void eventReferences.suggest!(eventQuery, controller.signal).then(events => {
        if (!controller.signal.aborted) setEventSuggestions({ query: eventQuery, events });
      }, () => { if (!controller.signal.aborted) setEventSuggestionError(true); });
    }, 150);
    return () => { clearTimeout(timer); controller.abort(); };
  }, [eventQuery, eventReferences?.suggest, state.disabled, state.busy]);
  const candidateEvents = [...new Map([...(eventReferences?.events ?? []), ...(eventSuggestions && eventSuggestions.query === eventQuery ? eventSuggestions.events : [])].map(event => [event.id, event])).values()];
  const matches: readonly PreviewMention[] = state.disabled || state.busy ? [] : eventQuery !== undefined
    ? candidateEvents.filter(event => !event.cancelled && (event.code.startsWith(eventQuery) || event.aliases.some(code => code.startsWith(eventQuery))))
      .sort((left, right) => left.title.localeCompare(right.title)).slice(0, 12)
      .map(event => ({ id: event.id, name: event.title, handle: event.code, eventCode: event.code, expandsDirect: false }))
    : query === undefined ? [] : host.mentionCandidates(target)
    .filter(person => person.handle.toLocaleLowerCase().startsWith(query))
    .sort((left, right) => left.name.localeCompare(right.name));
  const selectedIndex = Math.min(selected, Math.max(0, matches.length - 1));
  const restoreCaret = () => {
    const element = input();
    if (!element) return;
    element.focus({ preventScroll: true });
    if (pendingSelection.current !== null) {
      element.setSelectionRange(pendingSelection.current, pendingSelection.current);
      updateSelection(element);
      pendingSelection.current = null;
    }
  };
  const replaceSelection = (text: string, start = selection.current.start, end = selection.current.end) => {
    if (state.disabled || state.busy) return;
    const value = host.composer(target).value;
    pendingSelection.current = start + text.length;
    setDismissed(true);
    host.changeDraft(target, value.slice(0, start) + text + value.slice(end));
    // The host may synchronously publish its new snapshot or schedule a render.
    // Layout effect applies the same caret after the controlled value is committed.
    queueMicrotask(restoreCaret);
  };
  const selectMention = (person: PreviewMention) => {
    if (person.eventCode && caret !== null && eventQuery !== undefined) {
      replaceSelection(`#arr:${person.eventCode} `, caret - eventQuery.length - 5, caret);
    } else if (person.expandsDirect) {
      setDismissed(true);
      setExpansion(person);
    } else if (caret !== null && query !== undefined) {
      replaceSelection(`@${person.handle} `, caret - query.length - 1, caret);
    }
  };
  useEffect(() => () => closeEmoji.current?.(), []);
  useLayoutEffect(() => {
    const element = input();
    if (!element) return;
    const selected = () => updateSelection(element);
    // Native selection notifications also cover programmatic range changes and
    // mobile selection handles, which React's synthetic onSelect can miss.
    element.addEventListener("select", selected);
    element.addEventListener("selectionchange", selected);
    return () => {
      element.removeEventListener("select", selected);
      element.removeEventListener("selectionchange", selected);
    };
  }, []);
  useLayoutEffect(() => {
    if (target.parentMessageId) input()?.focus({ preventScroll: true });
  }, [target.parentMessageId]);
  useLayoutEffect(() => {
    if (pendingSelection.current !== null) restoreCaret();
  }, [state.value]);
  useLayoutEffect(() => {
    const element = input();
    if (!element) return;
    element.setAttribute("aria-autocomplete", "list");
    element.setAttribute("aria-controls", listId);
    element.setAttribute("aria-expanded", String(matches.length > 0));
    if (matches.length) element.setAttribute("aria-activedescendant", `${listId}-${selectedIndex}`);
    else element.removeAttribute("aria-activedescendant");
  });
  useLayoutEffect(() => {
    if (!state.busy && !state.disabled && restoreFocus.current) input()?.focus({ preventScroll: true });
  }, [state.busy, state.disabled]);
  return <div ref={container}
    onSelectCapture={event => {
      if (event.target instanceof HTMLTextAreaElement) updateSelection(event.target);
    }}
    onFocusCapture={event => { restoreFocus.current = event.target instanceof HTMLTextAreaElement; }}
    onBlurCapture={event => { if (event.relatedTarget) restoreFocus.current = false; }}
    onCompositionStartCapture={() => { composing.current = true; setDismissed(true); }}
    onCompositionEndCapture={() => { composing.current = false; setDismissed(false); const field = input(); if (field) updateSelection(field); }}
    onKeyDownCapture={event => {
      // A modal's native Escape must not also close its enclosing thread.
      if (event.key === "Escape" && event.target instanceof Element && event.target.closest("dialog")) {
        event.stopPropagation();
        return;
      }
      if (!(event.target instanceof HTMLTextAreaElement)) return;
      if (composing.current || event.nativeEvent.isComposing || event.keyCode === 229) {
        if (event.key === "Enter") event.stopPropagation();
        return;
      }
      if (matches.length && ["ArrowDown", "ArrowUp", "Enter", "Tab", "Escape"].includes(event.key)) {
        event.preventDefault();
        event.stopPropagation();
        if (event.key === "Escape") setDismissed(true);
        else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
          setSelected((selectedIndex + (event.key === "ArrowDown" ? 1 : -1) + matches.length) % matches.length);
        } else selectMention(matches[selectedIndex]!);
      }
    }} onPasteCapture={event => {
      const files = [...event.clipboardData.files].filter(file => /^(image|video)\//.test(file.type));
      const emoji = pastedEmoji(event.clipboardData.getData("text/plain"));
      if (!files.length && emoji && !state.disabled && !state.busy) {
        setEmojiSaveError("");
        void host.savedEmojis.save(emoji).catch(error => setEmojiSaveError(`Emoji er sett inn, men kunne ikkje lagrast: ${error instanceof Error ? error.message : String(error)}. Bruk Eigen emoji i veljaren for å prøve igjen.`));
      }
      if (files.length) {
        event.preventDefault();
        if (!state.disabled && !state.busy) host.upload(target, files);
      }
    }}>
    <style>{eventContextCss}</style>
    <DraftComposer hasAttachments={state.media.length > 0} label={target.parentMessageId ? "Svar i tråden" : "Skriv melding"}
      value={state.value} onChange={value => {
        const field = input();
        if (field) updateSelection(field);
        setDismissed(false);
        setSelected(0);
        setEventSendError("");
        host.changeDraft(target, value);
      }}
      onSend={() => {
        if (composing.current || expansion) return;
        const draft = host.composer(target);
        if (draft.disabled || draft.busy || (!draft.value.trim() && !draft.media.length)) return;
        if (activeEventContext) {
          let prepared: string;
          try { prepared = messageWithEventContext(draft.value, activeEventContext); }
          catch (error) { setEventSendError(error instanceof Error ? error.message : "Arrangementskonteksten kunne ikkje leggjast til."); return; }
          if (prepared !== draft.value) {
            preparedEventDraft.current = { before: draft.value, after: prepared };
            // This token came from the explicit context, not from typing a
            // partial code. A restored failed draft must remain ready to retry.
            setDismissed(true);
            host.changeDraft(target, prepared);
          }
          // Hosts own the draft. Never submit stale text if a host declined it.
          if (host.composer(target).value !== prepared) return;
        }
        host.send(target);
      }} disabled={state.disabled} busy={state.busy}
      sendOnEnter={state.sendOnEnter} error={eventSendError || state.error}
      hint={state.sendOnEnter ? "Enter sender · Shift+Enter gir ny linje" : "Bruk Send for å sende"}
      leading={<>
        {activeEventContext && <div className="sp-event-compose-context" role="group" aria-label="Arrangement for meldinga">
          <span>Skriv om <strong title={`#arr:${activeEventContext.code}`}>{activeEventContext.title}</strong></span>
          <Button variant="quiet" disabled={state.disabled || state.busy} onClick={() => { setEventSearch(""); setEventPickerOpen(true); }}>Byt arrangement</Button>
          <Button variant="quiet" aria-label="Fjern arrangement frå meldinga" disabled={state.disabled || state.busy} onClick={() => chooseEventContext(null)}>Fjern</Button>
        </div>}
        {emojiSaveError && <Status tone="error">{emojiSaveError}</Status>}
        {eventQuery && eventSuggestionError && <Status tone="error">Arrangementsforslaga kunne ikkje lastast. Du kan framleis skrive koden eller opne Arrangement-fana.</Status>}
        {matches.length > 0 && <div id={listId} role="listbox" aria-label={eventQuery !== undefined ? "Arrangementsforslag" : "Omtaleforslag"}>
          {matches.map((person, index) => <Button key={person.id} id={`${listId}-${index}`} role="option"
            aria-selected={index === selectedIndex} onPointerDown={event => event.preventDefault()}
            onClick={() => selectMention(person)}>
            {person.name} · {person.eventCode ? "#arr:" : "@"}{person.handle}{person.expandsDirect ? " · Ny gruppesamtale" : ""}
          </Button>)}
        </div>}
        {(state.media.length > 0 || state.uploadStatus) && <PreviewAttachments media={state.media} status={state.uploadStatus}
          busy={state.busy || state.disabled} onRemove={id => host.removeMedia(target, id)} />}
      </>}
      tools={<div className="sp-writing-tools" role="toolbar" aria-label="Skriveverktøy">
        {eventContextAvailable && <Button className="sp-composer-symbol" variant="quiet" aria-label="Skriv om arrangement" title="Skriv om arrangement" disabled={state.disabled || state.busy} onClick={() => { setEventSearch(""); setEventPickerOpen(true); }}>
          <svg aria-hidden="true" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"><rect x="3" y="5" width="18" height="16" rx="2" /><path d="M16 3v4M8 3v4M3 11h18M8 15h3" /></svg>
        </Button>}
        <Button className="sp-composer-symbol" variant="quiet" aria-label="Set inn emoji" title="Set inn emoji" disabled={state.disabled || state.busy} onClick={event => {
          const field = input();
          if (field) updateSelection(field);
          closeEmoji.current?.();
          closeEmoji.current = openEmojiPicker(event.currentTarget, host.savedEmojis, {
            title: "Set inn emoji",
            onSelect: emoji => replaceSelection(emoji)
          });
        }}><span aria-hidden="true">☺</span></Button>
        <Button className="sp-composer-symbol" variant="quiet" aria-label="Omtal ein person" title="Omtal ein person" disabled={state.disabled || state.busy} onClick={() => {
          const prefix = selection.current.start > 0 && !/\s/u.test(state.value[selection.current.start - 1] ?? "") ? " @" : "@";
          replaceSelection(prefix);
          setDismissed(false);
        }}><span aria-hidden="true">@</span></Button>
        <Button className="sp-composer-symbol" variant="quiet" aria-label="Legg ved bilete eller video" title="Legg ved bilete eller video" disabled={state.disabled || state.busy} onClick={() => fileInput.current?.click()}><span aria-hidden="true">📎</span></Button>
        <input ref={fileInput} type="file" accept="image/*,video/*" multiple hidden aria-label="Vel bilete eller video"
          onChange={event => {
            const files = [...(event.currentTarget.files ?? [])];
            event.currentTarget.value = "";
            if (!state.disabled && !state.busy) host.upload(target, files);
          }} />
        <Button className="sp-composer-symbol" variant="quiet" aria-label="Biletegenerering" title="Biletegenerering" onClick={host.openImageGeneration}><span aria-hidden="true">✦</span></Button>
        <Button className="sp-composer-symbol" variant="quiet" aria-label="Del posisjon med agent" title="Del posisjon med agent"
          disabled={state.disabled || state.busy} onClick={() => setLocationOpen(true)}><span aria-hidden="true">⌖</span></Button>
      </div>} />
    {eventPickerOpen && eventContextAvailable && <Dialog open title="Vel arrangement for meldinga" closeLabel="Avbryt" onClose={() => { setEventPickerOpen(false); queueMicrotask(restoreCaret); }}>
      <div className="sp-event-compose-picker">
        <label>Søk etter arrangement<input autoFocus type="search" maxLength={160} value={eventSearch} onChange={event => setEventSearch(event.currentTarget.value)} /></label>
        {pickerLoading && <Status>Lastar arrangement …</Status>}
        {pickerError && <Status tone="error">{pickerError} <Button onClick={() => setPickerAttempt(value => value + 1)}>Prøv igjen</Button></Status>}
        {!pickerLoading && !pickerError && !pickerEvents.length && <p>Ingen arrangement funne. Prøv eit anna namn eller ein kode.</p>}
        <div className="sp-event-compose-options">
          {pickerEvents.map(event => <Button key={event.id} title={`#arr:${event.code}`} aria-pressed={activeEventContext?.id === event.id} onClick={() => chooseEventContext(event)}>
            <span><strong>{event.title}</strong><span className="sp-event-compose-option-time">{eventContextWhen(event)}</span></span>
          </Button>)}
        </div>
        <Button variant="quiet" onClick={() => chooseEventContext(null)}>Vanleg kanalmelding</Button>
      </div>
    </Dialog>}
    {locationOpen && <AgentLocationSharingDialog api={host.agentLocations} channelId={target.channelId} channelName="denne kanalen"
      onClose={() => { setLocationOpen(false); queueMicrotask(restoreCaret); }} />}
    {expansion && <Dialog open title="Ny gruppesamtale" closeLabel="Avbryt" onClose={() => { setExpansion(null); queueMicrotask(restoreCaret); }}>
      <p>Start ei ny gruppesamtale med {expansion.name}? Den gamle direkte samtalen held fram privat.</p>
      <Button onClick={() => { host.expandDirect(target, expansion.id); setExpansion(null); }}>Start gruppesamtale</Button>
    </Dialog>}
  </div>;
}
