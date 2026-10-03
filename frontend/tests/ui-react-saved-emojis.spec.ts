import { expect, test, type BrowserContext, type Page } from "@playwright/test";

test.setTimeout(45_000);
const emoji = "🧑🏽‍🚀";
const preview = (page: Page) => page.locator("#sproyt-react-preview");
const picker = (page: Page, title = "Set inn emoji") => preview(page).getByRole("dialog", { name: title, exact: true });

async function emojiStore(context: BrowserContext) {
  const choices = new Map<string, string[]>();
  let failSave = false;
  await context.route(/\/api\/v1\/me\/emojis(?:\?|$)/, async route => {
    const actor = new URL(route.request().url()).searchParams.get("participant") ?? "guest";
    const items = choices.get(actor) ?? [];
    const method = route.request().method();
    if (method === "GET") { await route.fulfill({ json: items }); return; }
    if (method === "POST" && failSave) { await route.fulfill({ status: 503, body: "Prøve: lagring er utilgjengeleg" }); return; }
    const value = route.request().postDataJSON().emoji as string;
    choices.set(actor, method === "DELETE" ? items.filter(item => item !== value) : [...new Set([...items, value])]);
    await route.fulfill({ status: 204 });
  });
  return { choices, fail: (value: boolean) => { failSave = value; } };
}

async function openPicker(page: Page) {
  const control = preview(page).getByRole("button", { name: "Set inn emoji", exact: true });
  if (!await control.isVisible()) await preview(page).getByRole("button", { name: "Skriveverktøy", exact: true }).click();
  await control.click();
}

async function openComposer(page: Page, actor: string) {
  await page.goto(`/?participant=${actor}`);
  const field = preview(page).getByRole("textbox", { name: "Skriv melding", exact: true });
  await expect(field).toBeEnabled();
  await field.fill("Bevar utkast ");
  await openPicker(page);
  return field;
}

test("saved personal emoji is shared by composer and reactions across pages, and removing it leaves history", async ({ page, context }) => {
  await emojiStore(context);
  const actor = `saved-emoji-${Date.now()}`;
  const field = await openComposer(page, actor);
  await picker(page).locator("summary", { hasText: "Eigen emoji" }).click();
  await picker(page).getByRole("textbox", { name: "Lim inn Unicode-emoji" }).fill(emoji);
  await picker(page).getByRole("button", { name: "Bruk emoji", exact: true }).click();
  await expect(field).toHaveValue(`Bevar utkast ${emoji}`);
  await expect(field).toBeFocused();
  await preview(page).getByRole("button", { name: /^Send(?:\s|$)/ }).click();
  const card = preview(page).locator(".sp-channel-pane [data-message-id]").filter({ hasText: `Bevar utkast ${emoji}` }).last();
  await expect(card).toBeVisible();
  const add = card.getByRole("button", { name: "Legg til reaksjon", exact: true });
  await add.click();
  const reaction = picker(page, "Reager på meldinga");
  const saved = reaction.getByRole("button", { name: `Bruk lagra ${emoji}`, exact: true });
  await expect(saved).toBeVisible();
  await reaction.getByRole("button", { name: "Fleire emoji", exact: true }).click();
  await reaction.getByRole("searchbox", { name: "Finn emoji", exact: true }).fill("unmatched");
  await expect(saved).toHaveCount(0);
  await reaction.getByRole("button", { name: "Færre emoji", exact: true }).click();
  await expect(saved).toBeVisible();
  await saved.focus(); await saved.press("ArrowUp"); await expect(saved).toBeFocused();
  await saved.press("Enter");
  await expect(card.getByRole("button", { name: `${emoji}: 1 reaksjonar`, exact: true })).toBeVisible();
  await expect(add).toBeFocused();

  const second = await context.newPage();
  const secondField = await openComposer(second, actor);
  const savedChoice = picker(second).getByRole("button", { name: `Bruk lagra ${emoji}`, exact: true });
  await expect(savedChoice).toBeVisible();
  const bounds = await savedChoice.boundingBox();
  expect(bounds!.height).toBeGreaterThanOrEqual(44);
  expect(bounds!.width).toBeGreaterThanOrEqual(43);
  await savedChoice.click();
  await expect(secondField).toHaveValue(`Bevar utkast ${emoji}`);
  await openPicker(second);
  await picker(second).getByRole("button", { name: "Fjern lagra emoji", exact: true }).click();
  await picker(second).getByRole("button", { name: `Fjern ${emoji} frå lagra emoji`, exact: true }).click();
  await expect(picker(second).getByRole("button", { name: `Fjern ${emoji} frå lagra emoji`, exact: true })).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(card.getByRole("button", { name: `${emoji}: 1 reaksjonar`, exact: true })).toBeVisible();
  await add.click();
  await expect(reaction.getByRole("button", { name: `Bruk lagra ${emoji}`, exact: true })).toHaveCount(0);
  await expect(reaction.getByRole("button", { name: "Tommel opp, ja, bra", exact: true })).toBeVisible();
  await page.keyboard.press("Escape");
  await second.close();
});

test("saving errors retain input and draft, allow use without saving, and direct paste can be reused after reload", async ({ page, context }) => {
  const store = await emojiStore(context);
  store.fail(true);
  const actor = `emoji-error-${Date.now()}`;
  const field = await openComposer(page, actor);
  await picker(page).locator("summary", { hasText: "Eigen emoji" }).click();
  const custom = picker(page).getByRole("textbox", { name: "Lim inn Unicode-emoji" });
  await custom.fill(emoji);
  await picker(page).getByRole("button", { name: "Bruk emoji", exact: true }).click();
  await expect(picker(page)).toContainText("Kunne ikkje lagre emoji");
  await expect(custom).toHaveValue(emoji);
  await expect(field).toHaveValue("Bevar utkast ");
  await picker(page).getByRole("button", { name: "Bruk utan å lagre", exact: true }).click();
  await expect(field).toHaveValue(`Bevar utkast ${emoji}`);
  expect(store.choices.get(actor) ?? []).toEqual([]);
  store.fail(false);
  await field.focus();
  await field.evaluate(element => {
    const clipboardData = new DataTransfer(); clipboardData.setData("text/plain", "🇬🇷");
    element.dispatchEvent(new ClipboardEvent("paste", { bubbles: true, clipboardData }));
  });
  await expect.poll(() => store.choices.get(actor)).toEqual(["🇬🇷"]);
  await page.reload();
  await expect(field).toBeEnabled();
  await field.fill("Etter innlasting ");
  await openPicker(page);
  await picker(page).getByRole("button", { name: "Bruk lagra 🇬🇷", exact: true }).click();
  await expect(field).toHaveValue("Etter innlasting 🇬🇷");
});
