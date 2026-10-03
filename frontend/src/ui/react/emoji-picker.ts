import { openReactionPicker, reactionEmoji } from "@sproyt/ui/react";
import type { SavedEmojiApi } from "../../saved-emojis";

/** App-owned additions to the shared native popup, retaining its keyboard and focus contract. */
export function openEmojiPicker(anchor: HTMLElement, api: SavedEmojiApi, options: {
  title: string; closeLabel?: string; onSelect: (emoji: string) => void;
}): () => void {
  const close = openReactionPicker(anchor, { ...options, searchLabel: "Finn emoji",
    items: [...reactionEmoji, ["😀", "Stort smil, glad"]] });
  const dialog = anchor.closest(".sp-theme")?.querySelector<HTMLDialogElement>(".sp-reaction-picker");
  if (!dialog) return close;
  dialog.classList.add("sp-personal-emoji-picker");
  let emojis: string[] = [];
  let managing = false;
  let pending = false;
  const section = document.createElement("section");
  section.className = "sp-saved-emojis";
  section.setAttribute("aria-label", "Lagra emoji");
  const head = document.createElement("div");
  head.className = "sp-saved-emoji-head";
  const title = document.createElement("strong");
  title.textContent = "Lagra emoji";
  const manage = button("Fjern lagra emoji", "−", "sp-button");
  manage.setAttribute("aria-pressed", "false");
  const grid = document.createElement("div");
  grid.className = "sp-emoji-grid sp-saved-emoji-grid";
  const notice = document.createElement("p");
  notice.className = "sp-help";
  notice.setAttribute("role", "status");
  notice.textContent = "Hentar lagra emoji …";
  const retry = button("Prøv igjen", "Prøv igjen", "sp-button");
  retry.hidden = true;
  head.append(title, manage);
  section.append(head, grid, notice, retry);
  dialog.querySelector(".sp-reaction-head")?.after(section);
  const search = dialog.querySelector<HTMLInputElement>('input[type="search"]');
  const position = () => {
    if (!dialog.isConnected) return;
    const viewport = window.visualViewport;
    const top = (viewport?.offsetTop ?? 0) + 12;
    const bottom = (viewport?.offsetTop ?? 0) + (viewport?.height ?? innerHeight) - 12;
    const bounds = dialog.getBoundingClientRect();
    if (bounds.bottom > bottom) dialog.style.top = `${Math.max(top, bottom - bounds.height)}px`;
  };
  const draw = () => {
    if (!dialog.isConnected) return;
    grid.replaceChildren();
    manage.disabled = pending || !emojis.length;
    const query = search?.value.toLocaleLowerCase() ?? "";
    for (const emoji of emojis.filter(emoji => !query || emoji.toLocaleLowerCase().includes(query))) {
      const choice = button(managing ? `Fjern ${emoji} frå lagra emoji` : `Bruk lagra ${emoji}`, emoji, "sp-emoji");
      choice.disabled = pending;
      choice.onclick = () => {
        if (managing) void mutate(emoji, false);
        else { options.onSelect(emoji); close(); }
      };
      grid.append(choice);
    }
    position();
  };
  manage.onclick = () => {
    managing = !managing;
    manage.setAttribute("aria-pressed", String(managing));
    manage.setAttribute("aria-label", managing ? "Ferdig med å fjerne emoji" : "Fjern lagra emoji");
    manage.textContent = managing ? "✓" : "−";
    draw();
  };
  grid.addEventListener("keydown", event => {
    const choices = [...grid.querySelectorAll<HTMLButtonElement>("button")];
    const shift = { ArrowRight: 1, ArrowLeft: -1, ArrowDown: 6, ArrowUp: -6 }[event.key];
    if (shift !== undefined && choices.length) {
      event.preventDefault();
      const index = choices.indexOf(document.activeElement as HTMLButtonElement) + shift;
      choices[((index % choices.length) + choices.length) % choices.length]?.focus();
    }
  });
  const load = async () => {
    retry.hidden = true;
    notice.textContent = "Hentar lagra emoji …";
    try { emojis = await api.list(); notice.textContent = emojis.length ? "" : "Lim inn ein eigen emoji for å lagre han her."; draw(); }
    catch (error) { notice.textContent = `Kunne ikkje hente lagra emoji: ${message(error)}`; retry.hidden = false; }
    position();
  };
  retry.onclick = () => { void load(); };
  search?.addEventListener("input", draw);
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
  input.autocomplete = "off";
  input.placeholder = "🫶";
  label.append(input);
  const submit = button("Bruk emoji", "↵", "sp-button");
  submit.type = "submit";
  submit.disabled = true;
  const useOnly = button("Bruk utan å lagre", "Bruk utan å lagre", "sp-button");
  useOnly.disabled = true;
  useOnly.onclick = () => { if (input.value.trim()) { options.onSelect(input.value.trim()); close(); } };
  const error = document.createElement("p");
  error.className = "sp-help";
  error.setAttribute("role", "status");
  const mutate = async (emoji: string, saved: boolean) => {
    pending = true;
    input.disabled = true;
    submit.disabled = true;
    error.textContent = saved ? "Lagrar emoji …" : "Fjernar emoji …";
    draw();
    try {
      await api.save(emoji, saved);
      if (!dialog.isConnected) return;
      if (saved) { options.onSelect(emoji); close(); }
      else { emojis = emojis.filter(item => item !== emoji); error.textContent = "Emoji er fjerna frå samlinga. Tidlegare meldingar og reaksjonar er uendra."; }
    } catch (failure) { error.textContent = `Kunne ikkje ${saved ? "lagre" : "fjerne"} emoji: ${message(failure)}${saved ? " Du kan bruke han utan å lagre." : ""}`; }
    finally { pending = false; input.disabled = false; submit.disabled = !input.value.trim(); draw(); }
  };
  input.oninput = () => { submit.disabled = pending || !input.value.trim(); useOnly.disabled = !input.value.trim(); };
  form.onsubmit = event => { event.preventDefault(); if (!pending && input.value.trim()) void mutate(input.value.trim(), true); };
  custom.addEventListener("toggle", () => { if (custom.open) input.focus(); position(); });
  form.append(label, submit);
  custom.append(disclosure, form, useOnly, error);
  dialog.append(custom);
  dialog.addEventListener("close", () => search?.removeEventListener("input", draw), { once: true });
  void load();
  position();
  return close;
}

function button(label: string, text: string, className: string): HTMLButtonElement {
  const element = document.createElement("button");
  element.type = "button"; element.className = className; element.textContent = text;
  element.setAttribute("aria-label", label); element.title = label;
  return element;
}
function message(error: unknown): string { return error instanceof Error ? error.message : String(error); }
