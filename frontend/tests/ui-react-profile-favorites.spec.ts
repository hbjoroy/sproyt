import { destinationSurface } from "./support/destination-surface";
import { expect, test, type Page } from "@playwright/test";

async function enter(page: Page, participant: string) {
  await page.goto(`/?participant=${participant}`);
  const app = page.locator("#sproyt-react-preview");
  await expect(app.getByRole("textbox", { name: "Skriv melding" })).toBeEnabled({ timeout: 15_000 });
  return app;
}
test("profile stars persist across isolated browsers and removal is visible in a fresh session", async ({ browser, baseURL }) => {
  const contexts = await Promise.all([1, 2, 3].map(() => browser.newContext({ baseURL, serviceWorkers: "block", viewport: { width: 1280, height: 844 } })));
  const owner = `profile-favorites-${Date.now()}`;
  try {
    const first = await contexts[0]!.newPage(); let ownerId = "";
    first.on("websocket", socket => socket.on("framereceived", frame => { const event = JSON.parse(String(frame.payload)); if (event.type === "hello") ownerId = event.payload.participant_id; }));
    const app = await enter(first, owner);
    const names = [`Favoritt A ${Date.now()}`, `Favoritt B ${Date.now()}`];
    for (const name of names) {
      const settings = app.getByRole("button", { name: "Innstillingar", exact: true });

      await settings.click(); await destinationSurface(app, "Innstillingar", true).getByRole("searchbox", { name: "Søk i innstillingar" }).fill("Ny vennekrets"); await destinationSurface(app, "Innstillingar", true).getByRole("button", { name: "Ny vennekrets", exact: true }).click();
      const create = destinationSurface(app, "Ny vennekrets", true);
      await create.getByLabel("Namn på vennekrets").fill(name); await create.getByRole("button", { name: "Opprett vennekrets", exact: true }).click();
      await expect(create).toHaveCount(0);
      for (let depth = 0; depth < 4 && await app.locator("[data-workspace-page], dialog[open]").count(); depth++) {
        const title = (await app.locator("[data-workspace-page], dialog[open]").getByRole("heading").first().textContent())!;
        await first.keyboard.press("Escape"); await expect(destinationSurface(app, title, true)).toHaveCount(0);
      }
    }
    const group = app.locator("[data-conversation-group]").filter({ has: first.getByRole("heading", { name: names[0], exact: true }) });
    const groupId = (await group.getAttribute("data-conversation-group"))!;
    await first.evaluate(({ ownerId, groupId }) => localStorage.setItem(`sproyt:conversation-navigation:v1:${encodeURIComponent(ownerId)}`, JSON.stringify({ version: 1, entries: [{ id: groupId, pinned: true, uses: 0 }], othersExpanded: false })), { ownerId, groupId });
    await first.reload();
    await expect(app.getByRole("button", { name: `Fest ${names[0]} som favoritt`, exact: true })).toHaveAttribute("aria-pressed", "false");
    await app.getByText("Tidlegare lokale favorittar", { exact: true }).click();
    await app.getByRole("button", { name: "Overfør lokale favorittar", exact: true }).click();
    await expect(app.getByRole("button", { name: `Løys ${names[0]} frå favorittar`, exact: true })).toHaveAttribute("aria-pressed", "true");
    const second = await contexts[1]!.newPage(); const other = await enter(second, owner);
    await expect(other.getByRole("button", { name: `Løys ${names[0]} frå favorittar`, exact: true })).toHaveAttribute("aria-pressed", "true");
    await other.getByRole("button", { name: `Fest ${names[1]} som favoritt`, exact: true }).click();
    await expect(other.getByRole("button", { name: `Løys ${names[1]} frå favorittar`, exact: true })).toHaveAttribute("aria-pressed", "true");
    await contexts[0]!.close(); await contexts[1]!.close();
    const third = await contexts[2]!.newPage(); const fresh = await enter(third, owner);
    for (const name of names) await expect(fresh.getByRole("button", { name: `Løys ${name} frå favorittar`, exact: true })).toHaveAttribute("aria-pressed", "true");
    await fresh.getByRole("button", { name: `Løys ${names[0]} frå favorittar`, exact: true }).click();
    await expect(fresh.getByRole("button", { name: `Fest ${names[0]} som favoritt`, exact: true })).toHaveAttribute("aria-pressed", "false");
    await third.reload(); await expect(fresh.getByRole("button", { name: `Fest ${names[0]} som favoritt`, exact: true })).toHaveAttribute("aria-pressed", "false");
    await expect(fresh.getByRole("button", { name: `Løys ${names[1]} frå favorittar`, exact: true })).toHaveAttribute("aria-pressed", "true");
  } finally { await Promise.all(contexts.map(context => context.close())); }
});

for (const touch of [false, true]) test(`new-DM people rows stay compact without dividers (${touch ? "touch" : "pointer"})`, async ({ browser, baseURL }, info) => {
  const context = await browser.newContext({ baseURL, serviceWorkers: "block", viewport: { width: touch ? 320 : 1280, height: 844 }, hasTouch: touch, isMobile: touch });
  try {
    const page = await context.newPage();
    const names = ["Ada Lovelace", "Bert Eit veldig langt namn som skal halde seg på ei linje", "Cecilie", "Dag", "Eva"];
    await page.routeWebSocket(/\/ws(?:\?|$)/, socket => socket.onMessage(data => {
      const cmd = JSON.parse(String(data)); const reply = (type: string, payload: unknown) => socket.send(JSON.stringify({ protocol: "sproyt.chat.v1", type, payload, request_id: cmd.request_id }));
      if (cmd.type === "hello") reply("hello", { participant_id: "people-owner" });
      if (cmd.type === "list_users") reply("users_listed", { users: names.map((display_name, index) => ({ id: `person-${index}`, display_name, kind: "human", handle: `person${index}`, status_text: "Ein lang status", status_emoji: "", status_expires_at: null, external_provider: null, external_subject: null, created_at: "2026-01-01T00:00:00Z" })) });
      if (cmd.type === "list_my_circles") reply("circles_listed", { circles: [] });
      if (cmd.type === "list_my_channels") reply("channels_listed", { channels: [{ id: "channel", name: "General", slug: "general", kind: "public", circle_id: null, is_direct: false, direct_user_id: null, description: "", role: "member", last_read_sequence: 0, latest_sequence: 0 }] });
      if (cmd.type === "subscribe_channel") reply("subscription_started", { channel_id: "channel", history: [] });
      if (cmd.type === "load_recent_messages") reply("messages_loaded", { channel_id: "channel", messages: [] });
    }));
    await page.route(/\/api\/v1\/me\/favorite-circles(?:\?.*)?$/, route => route.fulfill({ json: [] }));
    const app = await enter(page, "people-compact");
    const back = app.getByRole("button", { name: "Samtalar", exact: true }); if (await back.isVisible()) await back.click();
    await app.getByRole("tab", { name: /^Direktemeldingar/ }).click(); await app.getByRole("button", { name: "Ny direktemelding", exact: true }).click();
    const dialog = destinationSurface(app, "Ny direktemelding", true);
    await expect(dialog.locator(".sp-direct-person")).toHaveCount(names.length);
    const rows = await dialog.locator(".sp-direct-person").evaluateAll(elements => elements.map(element => ({
      height: element.getBoundingClientRect().height, border: getComputedStyle(element).borderBottomWidth,
      overflow: element.scrollWidth > element.clientWidth,
      nameWhiteSpace: getComputedStyle(element.querySelector(".sp-direct-person-name")!).whiteSpace,
      detailWhiteSpace: getComputedStyle(element.querySelector(".sp-direct-person-detail")!).whiteSpace
    })));
    expect(rows.every(row => row.height >= (touch ? 44 : 36) && row.height <= 44.5 && row.border === "0px"
      && !row.overflow && row.nameWhiteSpace === "nowrap" && row.detailWhiteSpace === "nowrap")).toBe(true);
    await expect(dialog.getByRole("button", { name: `Start samtale med ${names[1]}`, exact: true })).toBeVisible();
    await page.screenshot({ path: info.outputPath(`compact-people-${touch ? "touch" : "pointer"}.png`) });
  } finally { await context.close(); }
});
