import { Button, Dialog, Status, openReactionPicker } from "@sproyt/ui/react";
import { useState, type ReactNode } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";
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
export function PreviewReactionActions({ message, host, open, primaryAction, overflowActions }: {
  message: ChatMessage; host: PreviewReactionHost; open: (message: ChatMessage, anchor: HTMLElement) => void;
  primaryAction?: ReactNode; overflowActions?: ReactNode;
}) {
  const [menuOpen, setMenuOpen] = useState(false);
  const reactions = host.reactions(message.id).filter(reaction => reaction.count > 0);
  // An empty Composer collapses its tool row on blur. Keep pointerdown from
  // moving focus until click, so that reflow cannot move a message action out
  // from under pointerup. Keyboard focus and touch scrolling remain native.
  const keepPointerTarget = (event: ReactPointerEvent<HTMLElement>) => {
    if (event.pointerType === "mouse" && event.button === 0) event.preventDefault();
  };
  if (message.deleted_at) return null;
  return <>
    <div className="sp-message-primary-actions">
      <Button className="sp-message-symbol" variant="quiet" aria-label="Legg til reaksjon" title="Legg til reaksjon"
        onPointerDown={keepPointerTarget} onClick={event => open(message, event.currentTarget)}><span aria-hidden="true">♡</span></Button>
      {primaryAction}
      <Button className="sp-message-symbol" variant="quiet" aria-label="Fleire meldingsval" title="Fleire meldingsval"
        onPointerDown={keepPointerTarget} onClick={() => setMenuOpen(true)}><span aria-hidden="true">⋯</span></Button>
      <Dialog open={menuOpen} title="Meldingsval" closeLabel="Lukk meldingsvala" onClose={() => setMenuOpen(false)}>
        <div className="sp-message-menu-dialog">
        {overflowActions}
        </div>
      </Dialog>
    </div>
    <div className="sp-message-reaction-row">
      {reactions.map(reaction => <Button key={reaction.emoji} aria-pressed={reaction.reactedByMe}
        title={reaction.names.join(", ")}
        onPointerDown={keepPointerTarget} aria-label={`${reaction.emoji}: ${reaction.count} reaksjonar`}
        onClick={() => host.toggleReaction(message.id, reaction.emoji)}>{reaction.emoji} {reaction.count}</Button>)}
      {reactions.length > 0 && <details className="sp-reaction-details"><summary title="Kven reagerte?" aria-label="Kven reagerte?">ⓘ</summary><ul>
        {reactions.map(reaction => <li key={reaction.emoji}>{reaction.emoji} {reaction.names.join(", ")}</li>)}
      </ul></details>}
      {host.reactionError(message.id) && <Status tone="error">{host.reactionError(message.id)}</Status>}
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
      // Extend the focused native popup with the app's existing arbitrary
      // Unicode reaction contract, preserving its Escape/focus restoration.
      const custom = document.createElement("details");
      custom.className = "sp-custom-reaction";
      const disclosure = document.createElement("summary");
      disclosure.textContent = "Eigen emoji";
      const form = document.createElement("form");
      const label = document.createElement("label");
      label.className = "sp-label";
      label.textContent = "Lim inn Unicode-emoji";
      const input = document.createElement("input");
      input.className = "sp-input";
      input.maxLength = 32;
      label.append(input);
      const submit = document.createElement("button");
      submit.type = "submit";
      submit.className = "sp-button";
      submit.textContent = "Bruk emoji";
      submit.disabled = true;
      input.addEventListener("input", () => { submit.disabled = !input.value.trim(); });
      form.append(label, submit);
      form.addEventListener("submit", event => {
        event.preventDefault();
        if (!input.value.trim()) return;
        host.toggleReaction(message.id, input.value.trim());
        close?.();
      });
      const keepInViewport = () => {
        if (!(dialog instanceof HTMLElement)) return;
        const viewport = window.visualViewport;
        const bottom = (viewport?.offsetTop ?? 0) + (viewport?.height ?? window.innerHeight) - 12;
        const bounds = dialog.getBoundingClientRect();
        if (bounds.bottom > bottom) dialog.style.top = `${Math.max((viewport?.offsetTop ?? 0) + 12, bottom - bounds.height)}px`;
      };
      custom.addEventListener("toggle", () => { if (custom.open) input.focus(); keepInViewport(); });
      custom.append(disclosure, form);
      dialog.append(custom);
      keepInViewport();
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
