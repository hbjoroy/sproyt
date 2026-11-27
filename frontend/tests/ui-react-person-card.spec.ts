import { expect, test, type Page } from "@playwright/test";

async function enter(page: Page, participant: string) {
  let account = "";
  let ordinal: number | null = null;
  page.on("websocket", socket => socket.on("framereceived", ({ payload }) => {
    const frame = JSON.parse(String(payload));
    if (frame.type === "hello") { account = frame.payload.participant_id; ordinal = frame.payload.signup_ordinal; }
  }));
  await page.goto(`/?participant=${participant}`);
  const app = page.locator("#sproyt-react-preview");
  await expect(app.getByRole("textbox", { name: "Skriv melding", exact: true })).toBeEnabled();
  await expect.poll(() => ordinal).toBeGreaterThan(0);
  return { app, account, ordinal };
}

test("two people see the same signup number and only their accessible conversations", async ({ page, browser }, { project }) => {
  test.setTimeout(60000);
  const authorName = `kort-${project.name}`;
  const author = await enter(page, authorName);
  const pane = author.app.locator(".sp-channel-pane");
  async function send(text: string) {
    await pane.getByRole("textbox", { name: "Skriv melding", exact: true }).fill(text);
    await pane.getByRole("button", { name: /^Send / }).click();
    const row = pane.locator("[data-message-id]").filter({ has: page.getByText(text, { exact: true }) });
    await expect(row).toBeVisible();
    return row;
  }
  const root = await send(`Felles frå ${authorName}`);
  const rootId = await root.getAttribute("data-message-id");
  await root.getByRole("button", { name: "Svar i tråd", exact: true }).click();
  const thread = author.app.locator(".sp-thread-pane");
  const replyText = `Trådsvar frå ${authorName}`;
  await thread.getByRole("textbox", { name: "Svar i tråden" }).fill(replyText);
  await thread.getByRole("button", { name: /^Send / }).click();
  await expect(thread.getByText(replyText, { exact: true })).toBeVisible();
  await thread.getByRole("button", { name: "Lukk tråden" }).click();
  const personalSpace = author.app.getByRole("button", { name: "Området mitt", exact: true });
  if (!await personalSpace.isVisible()) await author.app.getByRole("button", { name: "Samtalar", exact: true }).click();
  await personalSpace.click();
  await expect(author.app.getByRole("region", { name: "Området mitt", exact: true })).toBeVisible();
  await expect(pane.getByRole("textbox", { name: "Skriv melding" })).toBeEnabled();
  const secret = `Privat frå ${authorName}`;
  const own = await send(secret);
  await own.getByRole("button", { name: /^Vis personkort for / }).click();
  const myCard = page.getByRole("dialog");
  await expect(myCard.getByText(`Sprøyt-nummer ${author.ordinal}`)).toBeVisible();
  await expect(myCard.getByRole("link").first()).toContainText(secret);

  const other = await browser.newPage({ viewport: page.viewportSize()! });
  try {
    const participant = `lesar-${project.name}`;
    const reader = await enter(other, participant);
    const path = `/api/v1/people/${author.account}/card?participant=${participant}`;
    const denied = await other.request.get(path, { headers: { "x-sproyt-account": author.account } });
    expect(denied.status()).toBe(403);
    const result = await other.request.get(path, { headers: { "x-sproyt-account": reader.account } });
    expect(result.status()).toBe(200);
    expect(result.headers()["cache-control"]).toContain("no-store");
    expect(await result.text()).not.toContain(secret);
    const row = reader.app.locator(`.sp-channel-pane [data-message-id="${rootId}"]`);
    await row.getByRole("button", { name: /^Vis personkort for / }).click();
    const card = other.getByRole("dialog");
    await expect(card.getByText(`Sprøyt-nummer ${author.ordinal}`)).toBeVisible();
    await expect(card.getByRole("link")).toHaveCount(2);
    const reply = card.getByRole("link").filter({ hasText: replyText });
    await expect(reply).toHaveAttribute("href", new RegExp(`thread=${rootId}`));
    await reply.click();
    await expect(card).toHaveCount(0);
    await expect(reader.app.locator(".sp-thread-pane").getByText(replyText, { exact: true })).toBeVisible();
  } finally { await other.close(); }
});
