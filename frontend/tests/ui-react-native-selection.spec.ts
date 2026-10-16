import { expect, test, type Page } from "@playwright/test";

async function prepareMessage(page: Page, participant: string) {
  await page.goto(`/?participant=${participant}&ui=react`, { waitUntil: "domcontentloaded" });
  const preview = page.locator("#sproyt-react-preview");
  const composer = preview.getByRole("textbox", { name: "Skriv melding" });
  await expect(composer).toBeEnabled({ timeout: 15_000 });
  const body = `reaksjonstest ${participant} ${Date.now()}`;
  await composer.fill(body);
  await preview.getByRole("button", { name: "Send ↑", exact: true }).click();
  const message = preview.locator(".sp-channel-pane [data-message-id]").filter({ hasText: body });
  await expect(message).toBeVisible();
  return { preview, message, body, composer };
}

test("Native message text selection survives long press; explicit reactions work in threads", async ({ page, isMobile }) => {
  const errors: string[] = [];
  page.on("pageerror", error => errors.push(error.message));
  const { preview, message } = await prepareMessage(page, "playwright-react-gestures");
  const body = message.locator(".sp-message-body");
  const picker = preview.getByRole("dialog", { name: "Reager på meldinga" });
  // Neither native right-click nor the long press used by iOS text selection
  // may be consumed by a reaction dialog on the message body.
  expect(await body.evaluate(element => element.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true })))).toBe(true);
  const selected = await body.evaluate(element => {
    const range = document.createRange(); range.selectNodeContents(element);
    const selection = window.getSelection()!; selection.removeAllRanges(); selection.addRange(range);
    return selection.toString();
  });
  expect(selected.length).toBeGreaterThan(0);
  await body.dispatchEvent("pointerdown", { pointerType: "touch", clientX: 20, clientY: 20 });
  await page.waitForTimeout(650);
  await body.dispatchEvent("pointerup", { pointerType: "touch" });
  await expect(picker).toHaveCount(0);
  expect(await page.evaluate(() => window.getSelection()?.toString())).toBe(selected);
  await body.dispatchEvent("pointerdown", { pointerType: "touch", clientX: 20, clientY: 20 });
  await body.dispatchEvent("pointermove", { pointerType: "touch", clientX: 20, clientY: 60 });
  await page.waitForTimeout(550);
  await body.dispatchEvent("pointercancel", { pointerType: "touch" });
  await expect(picker).toHaveCount(0);
  await message.getByRole("button", { name: "Legg til reaksjon" }).click();
  await picker.getByRole("button", { name: "Tommel opp, ja, bra" }).click();
  await expect(message.getByRole("button", { name: "👍: 1 reaksjonar" })).toBeVisible();
  await message.getByRole("button", { name: "Svar i tråd" }).click();
  const thread = preview.locator(".sp-thread-pane");
  const reply = thread.getByRole("textbox", { name: "Svar i tråden" });
  const replyBody = `reaksjon på svar ${Date.now()}`;
  await reply.fill(replyBody);
  if (isMobile) await thread.getByRole("button", { name: "Send ↑", exact: true }).click();
  else await reply.press("Enter");
  await expect(reply).toHaveValue("");
  await expect(reply).toBeFocused();
  const replyMessage = thread.locator("[data-message-id]").filter({ hasText: replyBody });
  await replyMessage.getByRole("button", { name: "Legg til reaksjon" }).click();
  expect(errors).toEqual([]);
  await picker.getByRole("button", { name: "Feiring, hurra" }).click();
  await expect(replyMessage.getByRole("button", { name: "🎉: 1 reaksjonar" })).toBeVisible();
  await replyMessage.getByRole("button", { name: "Legg til reaksjon" }).click();
  await picker.locator("summary", { hasText: "Eigen emoji" }).click();
  await page.keyboard.press("Escape");
  await expect(thread).toBeVisible();
  await expect(picker).toBeHidden();
});
