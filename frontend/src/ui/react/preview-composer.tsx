import { Button, Composer, Dialog, Status, openReactionPicker, reactionEmoji } from "@sproyt/ui/react";
import { useEffect, useId, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import type { MediaObject } from "../../types";
import type { ComposerTarget } from "./host-adapter";
import { PreviewAttachments } from "./preview-media";

export interface PreviewMention {
  readonly id: string;
  readonly name: string;
  readonly handle: string;
  readonly expandsDirect: boolean;
}

export interface PreviewComposerState {
  readonly value: string;
  readonly disabled: boolean;
  readonly busy: boolean;
  readonly sendOnEnter: boolean;
  readonly media: readonly MediaObject[];
  readonly uploadStatus?: string;
  readonly error?: string;
}

export interface PreviewComposerHost {
  readonly composer: (target: ComposerTarget) => PreviewComposerState;
  readonly changeDraft: (target: ComposerTarget, value: string) => void;
  readonly send: (target: ComposerTarget) => void;
  readonly upload: (target: ComposerTarget, files: readonly File[]) => void;
  readonly removeMedia: (target: ComposerTarget, id: string) => void;
  readonly mentionCandidates: (target: ComposerTarget) => readonly PreviewMention[];
  readonly expandDirect: (target: ComposerTarget, userId: string) => void;
  readonly openImageGeneration: () => void;
}

/** A visible touch target makes tools beyond the compact row discoverable. */
function WritingTools({ children }: { readonly children: ReactNode }) {
  const track = useRef<HTMLDivElement>(null);
  const [overflow, setOverflow] = useState(false);
  const [atEnd, setAtEnd] = useState(false);
  const measure = () => {
    const element = track.current;
    if (!element) return;
    setOverflow(element.scrollWidth > element.clientWidth + 1);
    setAtEnd(element.scrollLeft + element.clientWidth >= element.scrollWidth - 2);
  };
  useLayoutEffect(() => {
    const element = track.current;
    if (!element) return;
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    measure();
    return () => observer.disconnect();
  }, []);
  return <div className="sp-writing-tools">
    <div className="sp-writing-tools-track" ref={track} onScroll={measure}>{children}</div>
    <Button className="sp-writing-tools-more" data-overflow={overflow}
      aria-label={atEnd ? "Vis første skriveverktøy" : "Vis fleire skriveverktøy"}
      title={atEnd ? "Vis første skriveverktøy" : "Vis fleire skriveverktøy"}
      onClick={() => {
        const element = track.current;
        if (element) element.scrollBy({ left: atEnd ? -element.scrollWidth : element.clientWidth * .85 });
      }}>{atEnd ? "←" : "→"}</Button>
  </div>;
}

/** Host state owns drafts and delivery; selection and transient suggestions
 * belong to the visible field. Hidden legacy fields never receive key events. */
export function PreviewComposer({ host, target }: {
  host: PreviewComposerHost; target: ComposerTarget;
}) {
  const composing = useRef(false);
  const container = useRef<HTMLDivElement>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const restoreFocus = useRef(false);
  const closeEmoji = useRef<(() => void) | undefined>(undefined);
  const selection = useRef({ start: 0, end: 0 });
  const pendingSelection = useRef<number | null>(null);
  const [caret, setCaret] = useState<number | null>(null);
  const [dismissed, setDismissed] = useState(false);
  const [selected, setSelected] = useState(0);
  const [expansion, setExpansion] = useState<PreviewMention | null>(null);
  const listId = useId();
  const state = host.composer(target);
  const input = () => container.current?.querySelector("textarea");
  const updateSelection = (element: HTMLTextAreaElement) => {
    selection.current = { start: element.selectionStart, end: element.selectionEnd };
    setCaret(element.selectionStart === element.selectionEnd ? element.selectionStart : null);
  };
  const match = !dismissed && !composing.current && caret !== null
    ? state.value.slice(0, caret).match(/(?:^|\s)@([\p{L}\p{N}_.-]*)$/u) : null;
  const query = match?.[1]?.toLocaleLowerCase();
  const matches = query === undefined || state.disabled || state.busy ? [] : host.mentionCandidates(target)
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
    if (person.expandsDirect) {
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
      if (files.length) {
        event.preventDefault();
        if (!state.disabled && !state.busy) host.upload(target, files);
      }
    }}>
    <Composer label={target.parentMessageId ? "Svar i tråden" : "Skriv melding"}
      value={state.value} onChange={value => {
        const field = input();
        if (field) updateSelection(field);
        setDismissed(false);
        setSelected(0);
        host.changeDraft(target, value);
      }}
      onSend={() => {
        if (composing.current || expansion) return;
        host.send(target);
      }} disabled={state.disabled} busy={state.busy}
      sendOnEnter={state.sendOnEnter} error={state.error}
      hint={state.sendOnEnter ? "Enter sender · Shift+Enter gir ny linje" : "Bruk Send for å sende"}
      leading={<>
        {matches.length > 0 && <div id={listId} role="listbox" aria-label="Omtaleforslag">
          {matches.map((person, index) => <Button key={person.id} id={`${listId}-${index}`} role="option"
            aria-selected={index === selectedIndex} onPointerDown={event => event.preventDefault()}
            onClick={() => selectMention(person)}>
            {person.name} · @{person.handle}{person.expandsDirect ? " · Ny gruppesamtale" : ""}
          </Button>)}
        </div>}
        {(state.media.length > 0 || state.uploadStatus) && <PreviewAttachments media={state.media} status={state.uploadStatus}
          busy={state.busy || state.disabled} onRemove={id => host.removeMedia(target, id)} />}
        {state.media.length > 0 && !state.value.trim() && <Button disabled={state.disabled || state.busy}
          onClick={() => host.send(target)}>Send vedlegg</Button>}
      </>}
      tools={<WritingTools>
        <Button disabled={state.disabled || state.busy} onClick={event => {
          const field = input();
          if (field) updateSelection(field);
          closeEmoji.current?.();
          closeEmoji.current = openReactionPicker(event.currentTarget, {
            title: "Set inn emoji", searchLabel: "Finn emoji",
            items: [...reactionEmoji, ["😀", "Stort smil, glad"]],
            onSelect: emoji => replaceSelection(emoji)
          });
        }}>Set inn emoji</Button>
        <Button disabled={state.disabled || state.busy} onClick={() => {
          const prefix = selection.current.start > 0 && !/\s/u.test(state.value[selection.current.start - 1] ?? "") ? " @" : "@";
          replaceSelection(prefix);
          setDismissed(false);
        }}>Omtal ein person</Button>
        <Button disabled={state.disabled || state.busy} onClick={() => fileInput.current?.click()}>Legg ved bilete eller video</Button>
        <input ref={fileInput} type="file" accept="image/*,video/*" multiple hidden aria-label="Vel bilete eller video"
          onChange={event => {
            const files = [...(event.currentTarget.files ?? [])];
            event.currentTarget.value = "";
            if (!state.disabled && !state.busy) host.upload(target, files);
          }} />
        <Button onClick={host.openImageGeneration}>Biletegenerering</Button>
      </WritingTools>} />
    {expansion && <Dialog open title="Ny gruppesamtale" closeLabel="Avbryt" onClose={() => { setExpansion(null); queueMicrotask(restoreCaret); }}>
      <p>Start ei ny gruppesamtale med {expansion.name}? Den gamle direkte samtalen held fram privat.</p>
      <Button onClick={() => { host.expandDirect(target, expansion.id); setExpansion(null); }}>Start gruppesamtale</Button>
    </Dialog>}
  </div>;
}
