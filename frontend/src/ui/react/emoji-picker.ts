import { reactionChoices } from "../../reaction-choices";
import { openReactionPicker, reactionEmoji } from "@sproyt/ui/react";
import type { SavedEmojiApi } from "../../saved-emojis";

/** App-owned additions to the shared native popup, retaining its keyboard and focus contract. */
export function openEmojiPicker(anchor: HTMLElement, api: SavedEmojiApi, options: {
  title: string; owner?: string | null; closeLabel?: string; onSelect: (emoji: string) => void;
}): () => void {
  let emojis: string[] = [];
  let managing = false;
  let pending = false;
  let loaded = false;
  const owner = options.owner;
  let choices = owner ? reactionChoices(owner, []) : [];
  const close = openReactionPicker(anchor, { ...options, searchLabel: "Finn emoji",
    items: [...reactionEmoji, ["😀", "Stort smil, glad"]],
    ...(owner && {
      moreLabel: "Fleire", lessLabel: "Færre", getItems: () => choices, keepOpen: () => managing,
      onSelect: (emoji: string) => {
        if (!managing) options.onSelect(emoji);
        else if (!pending && loaded) void changePin(emoji);
      },
    }),
  });
  const dialog = anchor.closest(".sp-theme")?.querySelector<HTMLDialogElement>(".sp-reaction-picker");
  if (!dialog) return close;
  dialog.classList.add("sp-personal-emoji-picker");
  if (owner) {
    dialog.classList.add("kr-reaction-picker");
    dialog.querySelector(".sp-reaction-head button")!.textContent = "Lukk";
  }
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
  const more = dialog.querySelector<HTMLButtonElement>(".sp-reaction-more");
  const catalog = dialog.querySelector<HTMLElement>(":scope > .sp-emoji-grid");
  const expanded = () => more?.getAttribute("aria-expanded") === "true";
  const pane = anchor.closest(".sp-thread-pane, .sp-channel-pane");
  const composer = pane?.querySelector(".sp-draft-composer");
  const position = () => {
    if (!dialog.isConnected) return;
    const viewport = window.visualViewport;
    const top = (viewport?.offsetTop ?? 0) + 12;
    const bottom = (viewport?.offsetTop ?? 0) + (viewport?.height ?? innerHeight) - 12;
    const bounds = dialog.getBoundingClientRect();
    if (owner && pane) {
      const room = pane.getBoundingClientRect();
      const left = (viewport?.offsetLeft ?? 0) + 12;
      const right = left + (viewport?.width ?? innerWidth) - 24;
      dialog.style.setProperty("--kr-picker-width", Math.min(420, room.width - 24, right - left) + "px");
      dialog.style.setProperty("--kr-picker-x", Math.max(left, Math.min(room.left + 12, right - bounds.width)) + "px");
      const floor = Math.min(bottom, composer?.getBoundingClientRect().top ?? room.bottom);
      dialog.style.setProperty("--kr-picker-y", Math.max(top, floor - bounds.height - 8) + "px");
      return;
    }
    if (bounds.bottom > bottom) dialog.style.top = `${Math.max(top, bottom - bounds.height)}px`;
  };
  const draw = () => {
    if (!dialog.isConnected) return;
    if (owner) {
      dialog.dataset.expanded = String(expanded());
      if (!expanded()) managing = false;
      dialog.dataset.pinning = String(managing);
      manage.hidden = !expanded();
      manage.disabled = pending || !loaded;
      manage.textContent = managing ? "Ferdig" : "Fest reaksjonar";
      manage.setAttribute("aria-label", manage.textContent);
      manage.setAttribute("aria-pressed", String(managing));
      close.refresh();
      for (const item of catalog?.querySelectorAll<HTMLButtonElement>("button") ?? []) {
        if (managing) {
          const pinned = emojis.includes(item.textContent ?? "");
          item.setAttribute("aria-pressed", String(pinned));
          item.setAttribute("aria-label", (pinned ? "Løys " : "Fest ") + item.textContent);
          item.disabled = pending;
        }
      }
      position();
      return;
    }
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
  async function changePin(emoji: string) {
    if (!owner) return;
    const save = !emojis.includes(emoji);
    pending = true;
    notice.textContent = "";
    draw();
    try {
      await api.save(emoji, save);
      if (!dialog?.isConnected) return;
      emojis = save ? [...emojis, emoji] : emojis.filter(item => item !== emoji);
      choices = reactionChoices(owner, emojis);
    } catch (failure) {
      notice.textContent = "Kunne ikkje endre festa reaksjonar: " + emojiErrorMessage(failure);
    } finally { pending = false; draw(); }
  }
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
    try { emojis = await api.list(); loaded = true; if (owner) choices = reactionChoices(owner, emojis); notice.textContent = owner || emojis.length ? "" : "Lim inn ein eigen emoji for å lagre han her."; draw(); }
    catch (error) { notice.textContent = `Kunne ikkje hente lagra emoji: ${emojiErrorMessage(error)}`; retry.hidden = false; }
    position();
  };
  retry.onclick = () => { void load(); };
  search?.addEventListener("input", draw);

  // Collapsing the native catalog clears search without emitting an input event.
  more?.addEventListener("click", draw);
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
    } catch (failure) { error.textContent = `Kunne ikkje ${saved ? "lagre" : "fjerne"} emoji: ${emojiErrorMessage(failure)}${saved ? " Du kan bruke han utan å lagre." : ""}`; }
    finally { pending = false; input.disabled = false; submit.disabled = !input.value.trim(); draw(); }
  };
  input.oninput = () => { submit.disabled = pending || !input.value.trim(); useOnly.disabled = !input.value.trim(); };
  form.onsubmit = event => { event.preventDefault(); if (!pending && input.value.trim()) void mutate(input.value.trim(), true); };
  custom.addEventListener("toggle", () => { if (custom.open) input.focus(); position(); });
  form.append(label, submit);
  custom.append(disclosure, form, useOnly, error);
  dialog.append(custom);
  const resize = owner ? new ResizeObserver(position) : null;
  resize?.observe(dialog);
  if (pane) resize?.observe(pane);
  if (composer) resize?.observe(composer);
  if (owner) window.visualViewport?.addEventListener("resize", position);
  draw();
  dialog.addEventListener("close", () => {
    resize?.disconnect();
    window.visualViewport?.removeEventListener("resize", position);
    search?.removeEventListener("input", draw);
    more?.removeEventListener("click", draw);
  }, { once: true });
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
function emojiErrorMessage(error: unknown): string { return error instanceof Error ? error.message : String(error); }
