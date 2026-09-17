import { expect, test, type Page } from "@playwright/test";

async function prepareMessage(page: Page, participant: string) {
  await page.goto(`/?participant=${participant}&ui=react`, { waitUntil: "domcontentloaded" });
  const preview = page.locator("#sproyt-react-preview");
  const composer = preview.getByRole("textbox", { name: "Skriv melding" });
  await expect(composer).toBeEnabled({ timeout: 15_000 });
  const body = `reaksjonstest ${participant} ${Date.now()}`;
  await composer.fill(body);
  await composer.press("Enter");
  const message = preview.locator(".sp-channel-pane [data-message-id]").filter({ hasText: body });
  await expect(message).toBeVisible();
  return { preview, message, body, composer };
}

test("React reactions support keyboard popup, several badges, Unicode and live server counts", async ({ page, context }) => {
  let sockets = 0;
  const commands: unknown[] = [];
  const errors: string[] = [];
  page.on("pageerror", error => errors.push(error.message));
  page.on("websocket", socket => {
    sockets++;
    socket.on("framesent", ({ payload }) => {
      const command = JSON.parse(String(payload));
      if (command.type === "toggle_message_reaction") commands.push(command.payload);
    });
  });
  const { preview, message, body } = await prepareMessage(page, "playwright-react-reactions");
  const add = message.getByRole("button", { name: "Legg til reaksjon", exact: true });
  await add.focus();
  await add.press("Enter");
  const picker = preview.getByRole("dialog", { name: "Reager på meldinga" });
  const thumb = picker.getByRole("button", { name: "Tommel opp, ja, bra" });
  await expect(thumb).toBeFocused();
  await thumb.press("ArrowRight");
  await expect(picker.getByRole("button", { name: "Hjarte, glad i, love" })).toBeFocused();
  await page.keyboard.press("ArrowLeft");
  await page.keyboard.press("Enter");
  await expect(message.getByRole("button", { name: "👍: 1 reaksjonar" })).toHaveAttribute("aria-pressed", "true");
  await expect(add).toBeFocused();
  await add.click();
  await picker.getByRole("button", { name: "Hjarte, glad i, love" }).click();
  await expect(message.getByRole("button", { name: "❤️: 1 reaksjonar" })).toHaveAttribute("aria-pressed", "true");
  await add.click();
  await expect(thumb).toHaveAttribute("aria-pressed", "true");
  await expect(picker.getByRole("button", { name: "Hjarte, glad i, love" })).toHaveAttribute("aria-pressed", "true");
  await page.keyboard.press("Escape");
  await expect(picker).toHaveCount(0);
  await expect(add).toBeFocused();

  await message.getByRole("button", { name: "Eigen emoji" }).click();
  const custom = preview.getByRole("dialog", { name: "Eigen reaksjon" });
  await custom.getByRole("textbox", { name: "Lim inn Unicode-emoji" }).fill("🦀");
  await custom.getByRole("button", { name: "Bruk emoji" }).click();
  await expect(message.getByRole("button", { name: "🦀: 1 reaksjonar" })).toBeVisible();

  const other = await context.newPage();
  await other.goto("/?participant=playwright-react-reactions-peer&ui=react", { waitUntil: "domcontentloaded" });
  const peerMessage = other.locator("#sproyt-react-preview [data-message-id]").filter({ hasText: body });
  await peerMessage.getByRole("button", { name: "👍: 1 reaksjonar" }).click();
  await expect(message.getByRole("button", { name: "👍: 2 reaksjonar" })).toHaveAttribute("aria-pressed", "true");
  await message.getByRole("button", { name: "👍: 2 reaksjonar" }).click();
  await expect(message.getByRole("button", { name: "👍: 1 reaksjonar" })).toHaveAttribute("aria-pressed", "false");
  await message.getByText("Kven reagerte?", { exact: true }).click();
  await expect(message.locator("details")).toContainText("❤️ Du");
  expect(commands).toHaveLength(4);
  expect(sockets).toBe(1);
  expect(errors).toEqual([]);
  await other.close();
});

test("React reaction gestures preserve touch scrolling and work in thread replies", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", error => errors.push(error.message));
  const { preview, message } = await prepareMessage(page, "playwright-react-gestures");
  const article = message.locator("article");
  await article.click({ button: "right" });
  const picker = preview.getByRole("dialog", { name: "Reager på meldinga" });
  await expect(picker).toBeVisible();
  await page.keyboard.press("Escape");
  await article.dispatchEvent("pointerdown", { pointerType: "touch", clientX: 20, clientY: 20 });
  await article.dispatchEvent("pointermove", { pointerType: "touch", clientX: 20, clientY: 60 });
  await page.waitForTimeout(550);
  await expect(picker).toHaveCount(0);
  await article.dispatchEvent("pointerup", { pointerType: "touch" });
  await article.dispatchEvent("pointerdown", { pointerType: "touch", clientX: 20, clientY: 20 });
  await expect(picker).toBeVisible();
  await article.dispatchEvent("pointerup", { pointerType: "touch" });
  await picker.getByRole("button", { name: "Tommel opp, ja, bra" }).click();
  await expect(message.getByRole("button", { name: "👍: 1 reaksjonar" })).toBeVisible();
  // Release the synthesized long-press click suppression before the next action.
  await article.dispatchEvent("click");
  await message.getByRole("button", { name: "Svar i tråd" }).click();
  const thread = preview.locator(".sp-thread-pane");
  const reply = thread.getByRole("textbox", { name: "Svar i tråden" });
  const replyBody = `reaksjon på svar ${Date.now()}`;
  await reply.fill(replyBody);
  await reply.press("Enter");
  await expect(reply).toHaveValue("");
  await expect(reply).toBeFocused();
  const replyMessage = thread.locator("[data-message-id]").filter({ hasText: replyBody });
  await replyMessage.getByRole("button", { name: "Legg til reaksjon" }).click();
  expect(errors).toEqual([]);
  await picker.getByRole("button", { name: "Feiring, hurra" }).click();
  await expect(replyMessage.getByRole("button", { name: "🎉: 1 reaksjonar" })).toBeVisible();
  await replyMessage.getByRole("button", { name: "Eigen emoji" }).click();
  await page.keyboard.press("Escape");
  await expect(thread).toBeVisible();
  await expect(preview.getByRole("dialog", { name: "Eigen reaksjon" })).toBeHidden();
});

test("React reaction failure leaves server badges intact and can be retried", async ({ page }) => {
  await page.addInitScript(() => {
    const NativeWebSocket = window.WebSocket;
    let fail = true;
    window.WebSocket = class extends NativeWebSocket {
      send(data: string): void {
        const command = JSON.parse(data);
        if (command.type === "toggle_message_reaction" && fail) {
          fail = false;
          setTimeout(() => this.dispatchEvent(new MessageEvent("message", { data: JSON.stringify({
            protocol: "sproyt.chat.v1", type: "error", request_id: command.request_id,
            payload: { code: "forbidden", message: "Reaksjonen vart avvist i prøva" }
          }) })), 20);
          return;
        }
        super.send(data);
      }
    };
  });
  const { preview, message } = await prepareMessage(page, "playwright-react-reaction-failure");
  const add = message.getByRole("button", { name: "Legg til reaksjon" });
  await add.click();
  await preview.getByRole("dialog", { name: "Reager på meldinga" }).getByRole("button", { name: "Tommel opp, ja, bra" }).click();
  await expect(message).toContainText("Reaksjonen vart avvist i prøva");
  await expect(message.getByRole("button", { name: "👍: 1 reaksjonar" })).toHaveCount(0);
  await add.click();
  await preview.getByRole("dialog", { name: "Reager på meldinga" }).getByRole("button", { name: "Tommel opp, ja, bra" }).click();
  await expect(message.getByRole("button", { name: "👍: 1 reaksjonar" })).toHaveAttribute("aria-pressed", "true");
  await expect(message).not.toContainText("Reaksjonen vart avvist i prøva");
});
