import { expect, test, type Page } from "@playwright/test";

async function enter(page: Page, participant: string) {
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

test("saving a thread reply produces one private log entry that opens the original thread", async ({ page }) => {
  const participant = "personal-saved-links";
  const { app, composer, headers } = await enter(page, participant);
  await composer.fill("Melding med eit svar som skal takast vare på");
  await composer.press("Enter");
  const root = app.locator(".sp-channel-pane [data-message-id]").filter({ hasText: "Melding med eit svar som skal takast vare på" });
  await root.getByRole("button", { name: "Svar i tråd", exact: true }).click();
  const rootId = await root.getAttribute("data-message-id");
  const thread = app.locator(".sp-thread-pane");
  const input = thread.getByRole("textbox", { name: "Svar i tråden" });
  await input.fill("Svar som skal takast vare på");
  await input.press("Enter");
  const reply = thread.locator("[data-message-id]").filter({ has: page.getByText("Svar som skal takast vare på", { exact: true }) });
  await reply.getByRole("button", { name: "Fleire meldingsval" }).click();
  await thread.getByRole("button", { name: "Ta vare på", exact: true }).click();
  await expect(thread.getByText("Lagra i Området mitt.", { exact: true })).toBeVisible();

  const sourceId = await reply.getAttribute("data-message-id");
  const duplicate = await page.request.post(`/api/v1/me/personal-space/saved-links/${sourceId}?participant=${participant}`, { headers });
  expect(await duplicate.json()).toEqual({ created: false });

  await app.getByRole("button", { name: "Området mitt", exact: true }).click();
  await expect(composer).toBeEnabled();
  const saved = app.getByRole("link", { name: "Lagra melding", exact: true });
  await expect(saved).toHaveCount(1);
  const href = new URL((await saved.getAttribute("href"))!, page.url());
  expect(href.searchParams.get("message")).toBe(sourceId);
  expect(href.searchParams.get("thread")).toBe(rootId);
  await saved.click();
  await expect(reply).toBeVisible();
});

test("the note expands on mobile, recovers its draft and resolves a concurrent edit", async ({ page }) => {
  const participant = "personal-note-editor";
  const { app, composer, headers } = await enter(page, participant);
  await app.getByRole("button", { name: "Området mitt", exact: true }).click();
  await expect(composer).toBeEnabled();
  await page.setViewportSize({ width: 390, height: 844 });
  const note = app.getByRole("region", { name: "Notat", exact: true });
  await note.getByRole("button", { name: "Rediger", exact: true }).click();
  const editor = note.getByRole("textbox", { name: "Notat i Markdown" });
  await editor.fill("# Mi sjekkliste\n\n- [x] Prøvd");
  await note.getByRole("button", { name: "Maksimer" }).click();
  await expect(composer).toBeHidden();
  await note.getByRole("button", { name: "Vis", exact: true }).click();
  await expect(note.getByRole("heading", { name: "Mi sjekkliste" })).toBeVisible();
  await note.getByRole("button", { name: "Minimer" }).click();
  await expect(composer).toBeVisible();

  const endpoint = `/api/v1/me/personal-space/note?participant=${participant}`;
  const otherEdit = await page.request.put(endpoint, { headers, data: { markdown: "Nyare versjon", revision: 0 } });
  expect(await otherEdit.json()).toEqual({ markdown: "Nyare versjon", revision: 1 });
  await page.reload();
  await expect(editor).toHaveValue("# Mi sjekkliste\n\n- [x] Prøvd");
  await expect(note.getByText(/Notatet vart endra ein annan stad/)).toBeVisible();
  await note.getByRole("button", { name: "Lagre mitt utkast" }).click();
  await expect(note.getByText("Notatet er lagra.", { exact: true })).toBeVisible();
  await page.reload();
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
