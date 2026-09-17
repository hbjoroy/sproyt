import { expect, test, type Page } from "@playwright/test";

// Development participants have no public handles. Supply handles on their
// genuine server profiles so the tests exercise the normal suggestion policy.
async function withHandles(page: Page) {
  await page.addInitScript(() => {
    const NativeWebSocket = window.WebSocket;
    window.WebSocket = class extends NativeWebSocket {
      constructor(url: string | URL, protocols?: string | string[]) {
        super(url, protocols);
        let dispatching = false;
        this.addEventListener("message", event => {
          if (dispatching) return;
          const frame = JSON.parse(String(event.data));
          if (!["users_listed", "circle_users_listed", "channel_users_listed"].includes(frame.type)) return;
          event.stopImmediatePropagation();
          frame.payload.users = frame.payload.users.map((user: { display_name: string }) => ({ ...user,
            handle: user.display_name.startsWith("mention-") ? user.display_name : null
          }));
          dispatching = true;
          this.dispatchEvent(new MessageEvent("message", { data: JSON.stringify(frame) }));
          dispatching = false;
        });
      }
    };
  });
}

async function enter(page: Page, participant: string, preview = true) {
  await page.goto(`/?participant=${participant}&ui=${preview ? "react" : "legacy"}`, { waitUntil: "domcontentloaded" });
  await expect(page.locator("#status")).toHaveText(/Tilkopla/, { timeout: 15_000 });
}

test("emoji replaces the current selection and restores the caret in channel and thread drafts", async ({ page }) => {
  await enter(page, "emoji-composer");
  const preview = page.locator("#sproyt-react-preview");
  const input = preview.getByRole("textbox", { name: "Skriv melding" });
  await expect(input).toBeEnabled();
  await input.fill("før gammal etter");
  await preview.getByRole("button", { name: "Skriveverktøy", exact: true }).click();
  await input.evaluate((field: HTMLTextAreaElement) => field.setSelectionRange(4, 10));
  await preview.getByRole("button", { name: "Set inn emoji", exact: true }).click();
  await preview.getByRole("dialog", { name: "Set inn emoji" }).getByRole("button", { name: "Tommel opp, ja, bra" }).click();
  await expect(input).toHaveValue("før 👍 etter");
  await expect(input).toBeFocused();
  await input.press("x");
  await expect(input).toHaveValue("før 👍x etter");
  await input.press("Enter");
  const root = preview.locator("[data-message-id]").filter({ hasText: "før 👍x etter" });
  await root.getByRole("button", { name: "Svar i tråd" }).click();
  const thread = preview.locator(".sp-thread-pane");
  const reply = thread.getByRole("textbox", { name: "Svar i tråden" });
  await reply.fill("trådtekst");
  await thread.getByRole("button", { name: "Skriveverktøy", exact: true }).click();
  await reply.press("Home");
  await thread.getByRole("button", { name: "Set inn emoji", exact: true }).click();
  await preview.getByRole("dialog", { name: "Set inn emoji" }).getByRole("button", { name: "Tommel opp, ja, bra" }).click();
  await expect(reply).toHaveValue("👍trådtekst");
  await expect(reply).toBeFocused();
  await expect(input).toHaveValue("");
});

test("mention arrows and Enter select before sending, Tab replaces at cursor, Escape dismisses and IME does not send", async ({ page, context }) => {
  const peer = await context.newPage();
  await enter(peer, "mention-anna", false);
  const secondPeer = await context.newPage();
  await enter(secondPeer, "mention-brita", false);
  await withHandles(page);
  const sends: string[] = [];
  page.on("websocket", socket => socket.on("framesent", ({ payload }) => {
    const command = JSON.parse(String(payload));
    if (command.type === "send_message") sends.push(command.payload.body);
  }));
  await enter(page, "composer-mentions");
  const preview = page.locator("#sproyt-react-preview");
  const input = preview.getByRole("textbox", { name: "Skriv melding" });
  await expect(input).toBeEnabled();
  await input.fill("Hei @mention-");
  await expect(preview.getByRole("option")).toHaveCount(2);
  await input.press("ArrowDown");
  await input.press("Enter");
  await expect(input).toHaveValue("Hei @mention-brita ");
  expect(sends).toEqual([]);
  await input.fill("før @mention-a etter");
  await input.evaluate((field: HTMLTextAreaElement) => field.setSelectionRange(14, 14));
  await expect(preview.getByRole("option", { name: /mention-anna/ })).toBeVisible();
  await input.press("Tab");
  await expect(input).toHaveValue("før @mention-anna  etter");
  await input.fill("@mention-a");
  await input.dispatchEvent("compositionstart");
  await input.dispatchEvent("keydown", { key: "Enter", code: "Enter", isComposing: true });
  expect(sends).toEqual([]);
  await input.dispatchEvent("compositionend");
  await expect(preview.getByRole("option")).toHaveCount(1);
  await input.press("Escape");
  await expect(preview.getByRole("option")).toHaveCount(0);
  await input.press("Enter");
  await expect.poll(() => sends).toEqual(["@mention-a"]);
  await expect(input).toHaveValue("");
  await expect(preview).toBeVisible();
  await preview.locator("[data-message-id]").filter({ hasText: "@mention-a" }).getByRole("button", { name: "Svar i tråd" }).click();
  const thread = preview.locator(".sp-thread-pane");
  const reply = thread.getByRole("textbox", { name: "Svar i tråden" });
  await reply.fill("Svar @mention-b");
  await reply.press("Tab");
  await expect(reply).toHaveValue("Svar @mention-brita ");
  expect(sends).toEqual(["@mention-a"]);
  await reply.press("Enter");
  await expect.poll(() => sends).toEqual(["@mention-a", "Svar @mention-brita"]);
  await expect(thread.getByText("Svar @mention-brita", { exact: true })).toBeVisible();
});

test("DM expansion requires confirmation and preserves the original private draft", async ({ page, context }) => {
  for (const name of ["mention-dm-peer", "mention-new-person"]) {
    const peer = await context.newPage();
    await enter(peer, name, false);
  }
  await withHandles(page);
  const expansions: { channel_id: string; user_id: string }[] = [];
  page.on("websocket", socket => socket.on("framesent", ({ payload }) => {
    const command = JSON.parse(String(payload));
    if (command.type === "expand_direct_channel") expansions.push(command.payload);
  }));
  await enter(page, "mention-dm-actor", false);
  await page.locator("#channel-people").click();
  await page.locator("#channel-member-list").getByRole("button", { name: "Start direktesamtale med mention-dm-peer", exact: true }).click();
  await expect(page.locator("#conversation-circle")).toHaveText("Direktemelding");
  await page.locator("#body").fill("privat utkast @mention-new");
  await enter(page, "mention-dm-actor");
  const preview = page.locator("#sproyt-react-preview");
  const input = preview.getByRole("textbox", { name: "Skriv melding" });
  await expect(input).toHaveValue("privat utkast @mention-new");
  await input.focus();
  await input.press("End");
  await expect(preview.getByRole("option", { name: /mention-new-person/ })).toBeVisible();
  await input.press("Enter");
  const dialog = preview.getByRole("dialog", { name: "Ny gruppesamtale" });
  await expect(dialog).toContainText("Den gamle direkte samtalen held fram privat.");
  expect(expansions).toEqual([]);
  await dialog.getByRole("button", { name: "Avbryt" }).click();
  await expect(input).toHaveValue("privat utkast @mention-new");
  await input.press("a");
  await input.press("Backspace");
  await input.press("Enter");
  await dialog.getByRole("button", { name: "Start gruppesamtale" }).click();
  await expect.poll(() => expansions.length).toBe(1);
  await expect(input).toHaveValue("");
  await expect(preview).toBeVisible();
  const original = preview.getByRole("button", { name: "mention-dm-peer · @mention-dm-peer", exact: true });
  await original.click();
  await expect(input).toHaveValue("privat utkast @mention-new");
});
