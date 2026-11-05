import { expect, test, type Page } from "@playwright/test";

async function enter(page: Page, participant: string) {
  await page.setViewportSize({ width: 1440, height: 900 });
  let account = "";
  page.on("websocket", socket => socket.on("framereceived", frame => {
    const event = JSON.parse(String(frame.payload));
    if (event.type === "hello") account = event.payload.participant_id;
  }));
  await page.goto(`/?participant=${participant}`);
  const app = page.locator("#sproyt-react-preview");
  const composer = app.getByRole("textbox", { name: "Skriv melding", exact: true });
  await expect(composer).toBeEnabled({ timeout: 15_000 });
  return { app, composer, headers: { "x-sproyt-account": account } };
}

test("saved thread replies show the current original and open its thread", async ({ page }, { project }) => {
  test.setTimeout(60_000);
  const participant = `personal-saved-links-${project.name}`;
  const message = `Melding med eit svar som skal takast vare på (${project.name})`;
  const { app, composer, headers } = await enter(page, participant);
  await composer.fill(message);
  await app.locator(".sp-channel-pane").getByRole("button", { name: /^Send / }).click();
  const root = app.locator(".sp-channel-pane [data-message-id]").filter({ hasText: message });
  await root.getByRole("button", { name: "Svar i tråd", exact: true }).click();
  const rootId = await root.getAttribute("data-message-id");
  const thread = app.locator(".sp-thread-pane");
  const input = thread.getByRole("textbox", { name: "Svar i tråden" });
  await input.fill("Svar som skal takast vare på");
  await thread.getByRole("button", { name: /^Send / }).click();
  const reply = thread.locator("[data-message-id]").filter({ has: page.getByText("Svar som skal takast vare på", { exact: true }) });
  await reply.getByRole("button", { name: "Fleire meldingsval" }).click();
  await thread.getByRole("button", { name: "Ta vare på", exact: true }).click();
  await expect(thread.getByText("Lagra i Området mitt.", { exact: true })).toBeVisible();

  const sourceId = await reply.getAttribute("data-message-id");
  const duplicate = await page.request.post(`/api/v1/me/personal-space/saved-links/${sourceId}?participant=${participant}`, { headers });
  expect(await duplicate.json()).toEqual({ created: false });

  await app.getByRole("button", { name: "Området mitt", exact: true }).click();
  await expect(composer).toBeEnabled();
  const quote = app.locator(".sp-saved-message-quote");
  await expect(quote).toHaveText("Svar som skal takast vare på");
  const saved = app.getByRole("link", { name: /Opne originalmeldinga.*Felles.*# General.*Tråd/ });
  await expect(saved).toHaveCount(1);
  const href = new URL((await saved.getAttribute("href"))!, page.url());
  expect(href.searchParams.get("message")).toBe(sourceId);
  expect(href.searchParams.get("thread")).toBe(rootId);
  await saved.click();
  await expect(reply).toBeVisible();

  await reply.getByRole("button", { name: "Fleire meldingsval" }).click();
  await thread.getByRole("button", { name: "Rediger", exact: true }).click();
  const edit = app.getByRole("dialog", { name: "Rediger melding" });
  await edit.getByRole("textbox", { name: "Melding", exact: true }).fill("**Oppdatert svar**");
  await edit.getByRole("button", { name: "Lagre", exact: true }).click();
  const edited = thread.locator(`[data-message-id="${sourceId}"]`);
  await expect(edited.getByText("Oppdatert svar", { exact: true })).toBeVisible();
  await edited.getByRole("button", { name: "Lukk meldingsvala" }).click();
  await app.getByRole("button", { name: "Området mitt", exact: true }).click();
  await expect(quote.locator("strong")).toHaveText("Oppdatert svar");

  // A pre-existing saved entry has the old Markdown link body.
  await composer.fill(`[Lagra melding](${href.pathname}${href.search})`);
  await app.locator(".sp-channel-pane").getByRole("button", { name: /^Send / }).click();
  await expect(quote.locator("strong")).toHaveText(["Oppdatert svar", "Oppdatert svar"]);

  await saved.first().click();
  await edited.getByRole("button", { name: "Fleire meldingsval" }).click();
  await thread.getByRole("button", { name: "Slett", exact: true }).click();
  await app.getByRole("dialog", { name: "Slett melding" }).getByRole("button", { name: "Slett melding" }).click();
  await expect(edited).toHaveCount(0);
  await app.getByRole("button", { name: "Området mitt", exact: true }).click();
  await expect(app.getByText("Originalmeldinga er utilgjengeleg.", { exact: true })).toHaveCount(2);
  await expect(quote).toHaveCount(0);
});

test("source messages require the current account and membership of the original channel", async ({ page, browser }, { project }) => {
  test.setTimeout(45_000);
  const participant = `personal-source-owner-${project.name}`;
  const { app, composer, headers } = await enter(page, participant);
  await app.getByRole("button", { name: "Området mitt", exact: true }).click();
  await expect(app.getByRole("heading", { name: "Området mitt", exact: true })).toBeVisible();
  await expect(composer).toBeEnabled();
  const body = "Ei privat melding";
  await composer.fill(body);
  await app.locator(".sp-channel-pane").getByRole("button", { name: /^Send / }).click();
  const original = app.locator(".sp-channel-pane [data-message-id]").filter({ has: page.getByText(body, { exact: true }) });
  await expect(original).toBeVisible();
  const channelId = new URL(page.url()).searchParams.get("channel");
  const id = await original.getAttribute("data-message-id");
  const sequence = await original.getAttribute("data-message-sequence");
  const endpoint = `/api/v1/me/personal-space/sources/${channelId}/${id}/${sequence}`;
  const allowed = await page.request.get(`${endpoint}?participant=${participant}`, { headers });
  expect(allowed.status()).toBe(200);
  expect(allowed.headers()["cache-control"]).toBe("private, no-store");
  expect(await allowed.json()).toMatchObject({ body });
  const wrongSequence = await page.request.get(
    `/api/v1/me/personal-space/sources/${channelId}/${id}/${Number(sequence) + 1}?participant=${participant}`, { headers });
  expect(wrongSequence.status()).toBe(404);

  const outsider = await browser.newPage();
  try {
    const otherParticipant = `personal-source-outsider-${project.name}`;
    const other = await enter(outsider, otherParticipant);
    const denied = await outsider.request.get(`${endpoint}?participant=${otherParticipant}`, { headers: other.headers });
    const wrongAccount = await outsider.request.get(`${endpoint}?participant=${otherParticipant}`, { headers });
    expect(denied.status()).toBe(404);
    expect(await denied.text()).not.toContain(body);
    expect(wrongAccount.status()).toBe(403);
  } finally {
    await outsider.close();
  }
});

test("the personal area puts the note beside chat and switches panes on compact screens", async ({ page }, { project }) => {
  const { app, composer } = await enter(page, `personal-responsive-layout-${project.name}`);
  await app.getByRole("button", { name: "Området mitt", exact: true }).click();
  const note = app.locator(".sp-personal-note");
  const timeline = app.locator(".sp-channel-pane .sp-timeline");
  const chatTab = app.getByRole("tab", { name: "Chatlogg", exact: true });
  const noteTab = app.getByRole("tab", { name: "Notat", exact: true });
  await expect(chatTab).toBeHidden();
  await expect(note).toBeVisible();
  await expect(timeline).toBeVisible();
  await expect(composer).toBeVisible();
  const noteAside = (await note.boundingBox())!;
  const chatAside = (await timeline.boundingBox())!;
  expect(noteAside.x).toBeGreaterThanOrEqual(chatAside.x + chatAside.width - 1);
  expect(noteAside.y).toBeLessThan(chatAside.y + chatAside.height);
  expect(noteAside.y + noteAside.height).toBeGreaterThan(chatAside.y);

  await note.getByRole("button", { name: "Maksimer", exact: true }).click();
  await expect(composer).toBeHidden();
  const noteExpanded = (await note.boundingBox())!;
  expect(noteExpanded.x).toBeLessThanOrEqual(chatAside.x + 1);
  expect(noteExpanded.x + noteExpanded.width).toBeGreaterThanOrEqual(noteAside.x + noteAside.width - 1);
  await note.getByRole("button", { name: "Minimer", exact: true }).click();
  await expect(composer).toBeVisible();
  expect((await note.boundingBox())!.x).toBeCloseTo(noteAside.x, 0);

  await note.getByRole("button", { name: "Maksimer", exact: true }).click();
  await app.locator(".sp-context").getByRole("button", { name: "Gå til siste", exact: true }).click();
  await expect(composer).toBeVisible();

  await page.setViewportSize({ width: 980, height: 900 });
  await expect(app.locator(".sp-sidebar")).toBeVisible();
  await expect(chatTab).toBeVisible();
  await page.setViewportSize({ width: 390, height: 844 });
  const chatPanel = app.getByRole("tabpanel", { name: "Chatlogg", exact: true });
  const notePanel = app.getByRole("tabpanel", { name: "Notat", exact: true });
  await expect(app.getByRole("tablist", { name: "Området mitt", exact: true })).toBeVisible();
  await chatTab.click();
  await expect(chatTab).toHaveAttribute("aria-selected", "true");
  await expect(noteTab).toHaveAttribute("aria-selected", "false");
  await expect(chatPanel).toBeVisible();
  await expect(notePanel).toBeHidden();
  await expect(chatTab).toHaveAttribute("aria-controls", (await chatPanel.getAttribute("id"))!);
  await composer.fill("Utkast i chatloggen");

  await chatTab.focus();
  await page.keyboard.press("ArrowRight");
  await expect(noteTab).toBeFocused();
  await expect(noteTab).toHaveAttribute("aria-selected", "true");
  await expect(chatTab).toHaveAttribute("aria-selected", "false");
  await expect(notePanel).toBeVisible();
  await expect(chatPanel).toBeHidden();
  await expect(composer).toBeHidden();
  await expect(noteTab).toHaveAttribute("aria-controls", (await notePanel.getAttribute("id"))!);
  await expect(note.getByRole("button", { name: "Maksimer", exact: true })).toBeHidden();
  await note.getByRole("button", { name: "Rediger", exact: true }).click();
  const editor = note.getByRole("textbox", { name: "Notat i Markdown" });
  await editor.fill("# Utkast i notatet");

  await noteTab.focus();
  await page.keyboard.press("ArrowLeft");
  await expect(chatTab).toBeFocused();
  await expect(composer).toBeVisible();
  await expect(composer).toHaveValue("Utkast i chatloggen");
  await expect(notePanel).toBeHidden();
  await noteTab.click();
  await expect(editor).toBeVisible();
  await expect(editor).toHaveValue("# Utkast i notatet");

  await chatTab.click();
  await page.setViewportSize({ width: 1440, height: 900 });
  await expect(composer).toBeVisible();
  await chatTab.waitFor({ state: "hidden" });
  await editor.focus();
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(noteTab).toHaveAttribute("aria-selected", "true");
  await expect(editor).toBeFocused();

  await page.setViewportSize({ width: 1440, height: 900 });
  await expect(composer).toBeVisible();
  await composer.focus();
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(chatTab).toHaveAttribute("aria-selected", "true");
  await expect(composer).toBeFocused();

  await noteTab.click();
  await app.getByRole("button", { name: /Området mitt.*Vis fullt namn og samtaleval/ }).click();
  await app.getByRole("group", { name: "Samtaleinformasjon" }).getByRole("button", { name: "Gå til siste" }).click();
  await expect(chatTab).toHaveAttribute("aria-selected", "true");
  await expect(composer).toHaveValue("Utkast i chatloggen");
});

test("the note recovers its draft and resolves a concurrent edit", async ({ page }, { project }) => {
  const participant = `personal-note-editor-${project.name}`;
  const { app, headers } = await enter(page, participant);
  await app.getByRole("button", { name: "Området mitt", exact: true }).click();
  await page.setViewportSize({ width: 390, height: 844 });
  const noteTab = app.getByRole("tab", { name: "Notat", exact: true });
  const note = app.locator(".sp-personal-note");
  await noteTab.click();
  await note.getByRole("button", { name: "Rediger", exact: true }).click();
  const editor = note.getByRole("textbox", { name: "Notat i Markdown" });
  await editor.fill("# Mi sjekkliste\n\n- [x] Prøvd");
  await note.getByRole("button", { name: "Vis", exact: true }).click();
  await expect(note.getByRole("heading", { name: "Mi sjekkliste" })).toBeVisible();

  const endpoint = `/api/v1/me/personal-space/note?participant=${participant}`;
  const otherEdit = await page.request.put(endpoint, { headers, data: { markdown: "Nyare versjon", revision: 0 } });
  expect(await otherEdit.json()).toEqual({ markdown: "Nyare versjon", revision: 1 });
  await page.reload();
  await noteTab.click();
  await expect(editor).toHaveValue("# Mi sjekkliste\n\n- [x] Prøvd");
  await expect(note.getByText(/Notatet vart endra ein annan stad/)).toBeVisible();
  await note.getByRole("button", { name: "Lagre mitt utkast" }).click();
  await expect(note.getByText("Notatet er lagra.", { exact: true })).toBeVisible();
  await page.reload();
  await noteTab.click();
  await expect(note.getByRole("heading", { name: "Mi sjekkliste" })).toBeVisible();

  const foreign = await page.request.get("/api/v1/me/personal-space/note?participant=personal-note-outsider", { headers });
  expect(foreign.status()).toBe(403);

  await page.evaluate(() => {
    const setItem = Storage.prototype.setItem;
    Storage.prototype.setItem = function (key, value) {
      if (this === sessionStorage && key.startsWith("sproyt.personal-note-draft.")) throw new Error("Storage unavailable");
      return setItem.call(this, key, value);
    };
  });
  await note.getByRole("button", { name: "Rediger", exact: true }).click();
  await editor.fill("Eit notat sjølv når lokal lagring feilar");
  const storageError = note.getByText(/Nettlesaren kunne ikkje ta vare på utkastet/);
  await expect(storageError).toBeVisible();
  await note.getByRole("button", { name: "Lagre", exact: true }).click();
  await expect(storageError).toHaveCount(0);
});
