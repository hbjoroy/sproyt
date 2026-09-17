import { Button, Dialog, Status, TextField, openReactionPicker } from "@sproyt/ui/react";
import { useState } from "react";
import type { PointerEvent } from "react";
import type { ChatMessage } from "../../types";

export interface PreviewReaction {
  readonly emoji: string;
  readonly count: number;
  readonly reactedByMe: boolean;
  readonly names: readonly string[];
}

export interface PreviewReactionHost {
  readonly reactions: (messageId: string) => readonly PreviewReaction[];
  readonly reactionError: (messageId: string) => string | undefined;
  readonly toggleReaction: (messageId: string, emoji: string) => void;
}

/** Presentation only: all counts, identities and mutations remain host-owned. */
export function PreviewReactionActions({ message, host, open }: {
  message: ChatMessage; host: PreviewReactionHost; open: (message: ChatMessage, anchor: HTMLElement) => void;
}) {
  const [customOpen, setCustomOpen] = useState(false);
  const [emoji, setEmoji] = useState("");
  const reactions = host.reactions(message.id).filter(reaction => reaction.count > 0);
  if (message.deleted_at) return null;
  // An empty Composer collapses its tool row on blur. Keep pointerdown from
  // moving focus until click, so that reflow cannot move a message action out
  // from under pointerup. Keyboard focus and touch scrolling remain native.
  const keepPointerTarget = (event: PointerEvent<HTMLElement>) => {
    if (event.pointerType === "mouse" && event.button === 0) event.preventDefault();
  };
  const submit = () => {
    if (!emoji.trim()) return;
    host.toggleReaction(message.id, emoji.trim());
    setCustomOpen(false);
  };
  return <>
    {reactions.map(reaction => <Button key={reaction.emoji} aria-pressed={reaction.reactedByMe}
      onPointerDown={keepPointerTarget}
      aria-label={`${reaction.emoji}: ${reaction.count} reaksjonar`}
      onClick={() => host.toggleReaction(message.id, reaction.emoji)}>
      {reaction.emoji} {reaction.count}
    </Button>)}
    <Button onPointerDown={keepPointerTarget} onClick={event => open(message, event.currentTarget)}>Legg til reaksjon</Button>
    <Button onPointerDown={keepPointerTarget} onClick={() => setCustomOpen(true)}>Eigen emoji</Button>
    {reactions.length > 0 && <details><summary onPointerDown={keepPointerTarget}>Kven reagerte?</summary><ul>
      {reactions.map(reaction => <li key={reaction.emoji}>{reaction.emoji} {reaction.names.join(", ")}</li>)}
    </ul></details>}
    {host.reactionError(message.id) && <Status tone="error">{host.reactionError(message.id)}</Status>}
    <div onKeyDown={event => { if (customOpen && event.key === "Escape") event.stopPropagation(); }}>
      <Dialog open={customOpen} title="Eigen reaksjon" closeLabel="Lukk reaksjonsdialogen" onClose={() => setCustomOpen(false)}>
        <form onSubmit={event => { event.preventDefault(); submit(); }}>
          <TextField label="Lim inn Unicode-emoji" maxLength={32} value={emoji}
            onChange={event => setEmoji(event.currentTarget.value)} />
          <Button type="submit" disabled={!emoji.trim()}>Bruk emoji</Button>
        </form>
      </Dialog>
    </div>
  </>;
}

export function createPreviewReactionPicker(host: PreviewReactionHost) {
  let close: (() => void) | undefined;
  let activeDialog: Element | null | undefined;
  return {
    close() { if (activeDialog?.isConnected) close?.(); close = undefined; activeDialog = undefined; },
    open(message: ChatMessage, anchor: HTMLElement) {
      if (message.deleted_at) return;
      if (activeDialog?.isConnected) close?.();
      // The package remembers the focused element. Give touch/context-menu
      // invocations the same visible focus-return target as keyboard activation.
      const control = anchor.matches("button") ? anchor : anchor.querySelector("button");
      if (control instanceof HTMLElement) control.focus({ preventScroll: true });
      close = openReactionPicker(anchor, {
        title: "Reager på meldinga", closeLabel: "Lukk reaksjonsveljaren",
        onSelect: emoji => host.toggleReaction(message.id, emoji)
      });
      // The starter picker has a single selected value; Sprøyt permits several.
      // Refresh every rendered choice, including after expanding/searching.
      const dialog = anchor.closest(".sp-theme")?.querySelector(".sp-reaction-picker");
      activeDialog = dialog;
      if (!dialog) return;
      const sync = () => {
        const selected = new Set(host.reactions(message.id).filter(item => item.reactedByMe).map(item => item.emoji));
        dialog.querySelectorAll(".sp-emoji").forEach(button => button.setAttribute("aria-pressed", String(selected.has(button.textContent ?? ""))));
      };
      sync();
      const observer = new MutationObserver(sync);
      observer.observe(dialog, { childList: true, subtree: true });
      dialog.addEventListener("close", () => observer.disconnect(), { once: true });
    }
  };
}
