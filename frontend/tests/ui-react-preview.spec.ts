import { readFileSync } from "node:fs";
import { expect, test } from "@playwright/test";
import { shouldMountReactInterface } from "../src/ui/react/development-selector";

test("React is the default interface with a local-only legacy escape hatch", () => {
  for (const host of ["localhost", "127.0.0.1", "[::1]"]) {
    expect(shouldMountReactInterface(new URL(`http://${host}/`))).toBe(true);
    expect(shouldMountReactInterface(new URL(`http://${host}/?ui=legacy`))).toBe(false);
    expect(shouldMountReactInterface(new URL(`http://${host}/?ui=react`))).toBe(true);
  }
  for (const host of ["chat.example.com", "localhost.example.com", "127.0.0.1.example.com"]) {
    expect(shouldMountReactInterface(new URL(`https://${host}/`))).toBe(true);
    expect(shouldMountReactInterface(new URL(`https://${host}/?ui=legacy`))).toBe(true);
  }
});

test("app constructs one runtime and preview only subscribes to it", () => {
  const app = readFileSync(new URL("../src/app.ts", import.meta.url), "utf8");
  const preview = readFileSync(new URL("../src/ui/react/development-preview.tsx", import.meta.url), "utf8");
  expect(app.match(/createApplicationRuntime\(/g)).toHaveLength(1);
  expect(app.match(/createConnectionController\(/g)).toHaveLength(1);
  expect(preview).not.toMatch(/new WebSocket|createApplicationRuntime\(|createConnectionController\(/);
  expect(preview).toContain("host.runtime.subscribe(update)");
});

test("default client mounts the design-system interface", async ({ page }) => {
  await page.goto("/?participant=playwright-preview-default", { waitUntil: "domcontentloaded" });
  await expect(page.locator("#status")).toHaveText(/Tilkopla/, { timeout: 15_000 });
  await expect(page.locator("#sproyt-react-preview")).toBeVisible();
  await expect(page.locator("#sproyt-react-preview").getByRole("textbox", { name: "Skriv melding" })).toBeEnabled();
  expect(await page.locator("#sproyt-app").evaluate((element: HTMLElement) => element.inert)).toBe(true);
  await expect(page.locator("#sproyt-app")).toBeHidden();
});

test("legacy element rules do not leak into the design-system shell", async ({ page }) => {
  await page.goto("/?participant=playwright-preview-style-boundary", { waitUntil: "domcontentloaded" });
  const preview = page.locator("#sproyt-react-preview");
  await expect(preview.getByRole("textbox", { name: "Skriv melding" })).toBeEnabled({ timeout: 15_000 });
  const styles = await preview.evaluate(root => {
    const main = root.querySelector<HTMLElement>(".sp-main")!;
    const masthead = root.querySelector<HTMLElement>(".sp-masthead")!;
    const input = root.querySelector<HTMLElement>(".sp-textarea")!;
    const mainStyle = getComputedStyle(main);
    const mastheadStyle = getComputedStyle(masthead);
    const inputStyle = getComputedStyle(input);
    return {
      mainDisplay: mainStyle.display,
      mainBackground: mainStyle.backgroundColor,
      mainBorder: mainStyle.borderTopWidth,
      mainRadius: mainStyle.borderTopLeftRadius,
      mainShadow: mainStyle.boxShadow,
      mastheadDisplay: mastheadStyle.display,
      inputRadius: inputStyle.borderTopLeftRadius
    };
  });
  expect(styles).toEqual({
    mainDisplay: "flex",
    mainBackground: "rgb(250, 249, 245)",
    mainBorder: "0px",
    mainRadius: "0px",
    mainShadow: "none",
    mastheadDisplay: "flex",
    inputRadius: "2px"
  });
});

test("local React preview follows the existing runtime and returns without reconnecting or losing drafts", async ({ page, context }) => {
  let sockets = 0;
  const pageErrors: string[] = [];
  page.on("websocket", () => sockets++);
  page.on("pageerror", error => pageErrors.push(error.message));
  await page.goto("/?participant=playwright-react-preview&ui=react", { waitUntil: "domcontentloaded" });
  const preview = page.locator("#sproyt-react-preview");
  await expect(preview).toBeVisible();
  await expect(page.locator("#status")).toHaveText(/Tilkopla/, { timeout: 15_000 });
  await expect(preview).toContainText("Førehandsvising for utvikling");
  await expect(preview.getByRole("button", { name: /# general/i })).toBeVisible();
  expect(sockets).toBe(1);
  expect(await page.locator("#sproyt-app").evaluate((element: HTMLElement) => element.inert)).toBe(true);
  await expect(preview.getByRole("textbox", { name: "Skriv melding" })).toBeEnabled();

  const sender = await context.newPage();
  await sender.goto("/?participant=playwright-preview-sender&ui=legacy", { waitUntil: "domcontentloaded" });
  await expect(sender.locator("#body")).toBeEnabled({ timeout: 15_000 });
  const liveMessage = `levande snapshot ${Date.now()}`;
  await sender.locator("#body").fill(liveMessage);
  await sender.locator("#send").click();
  await expect(preview.getByText(liveMessage, { exact: true })).toBeVisible();
  await sender.close();

  await preview.getByRole("textbox", { name: "Skriv melding" }).fill("utkast bevart gjennom førehandsvisinga");
  await preview.getByRole("searchbox", { name: "Finn samtale" }).fill("finnst-ikkje");
  await expect(preview).toContainText("Ingen samtalar funne.");
  await preview.getByRole("searchbox", { name: "Finn samtale" }).fill("");
  await expect(preview.getByRole("button", { name: /# general/i })).toBeVisible();
  await preview.getByRole("button", { name: "Meny", exact: true }).click();
  await preview.getByRole("button", { name: "Til fullt grensesnitt" }).first().click();
  await expect(preview).toHaveCount(0);
  await expect(page.locator("#body")).toHaveValue("utkast bevart gjennom førehandsvisinga");
  await expect(page.locator("#body")).toBeFocused();
  expect(await page.locator("#sproyt-app").evaluate((element: HTMLElement) => element.inert)).toBe(false);
  await expect(page.locator("#sproyt-app")).toBeVisible();
  expect(new URL(page.url()).searchParams.has("ui")).toBe(false);
  expect(sockets).toBe(1);
  expect(pageErrors).toEqual([]);
});

test("preview keyboard sending uses one host send and preserves Shift+Enter and IME", async ({ page }) => {
  const sends: string[] = [];
  page.on("websocket", socket => socket.on("framesent", ({ payload }) => {
    const command = JSON.parse(String(payload));
    if (command.type === "send_message") sends.push(command.payload.body);
  }));
  await page.goto("/?participant=playwright-preview-keyboard&ui=react", { waitUntil: "domcontentloaded" });
  const preview = page.locator("#sproyt-react-preview");
  const input = preview.getByRole("textbox", { name: "Skriv melding" });
  await expect(input).toBeEnabled({ timeout: 15_000 });
  const message = `preview keyboard ${Date.now()}`;
  await input.fill(message);
  await input.press("Shift+Enter");
  await expect(input).toHaveValue(`${message}\n`);
  await input.dispatchEvent("compositionstart");
  await input.dispatchEvent("keydown", { key: "Enter", code: "Enter", isComposing: true });
  expect(sends).toEqual([]);
  await input.dispatchEvent("compositionend");
  await input.press("Enter");
  await expect(input).toHaveValue("");
  const sent = preview.locator("[data-message-id]").filter({ hasText: message });
  await expect(sent.getByText(message, { exact: true })).toBeVisible();
  const timestamp = sent.locator("time");
  await timestamp.hover();
  await expect(sent.getByRole("tooltip")).toBeVisible();
  await timestamp.focus();
  await expect(sent.getByRole("tooltip")).toBeVisible();
  await input.focus();
  await expect(input).toBeFocused();
  expect(sends).toEqual([message]);
});

test("preview restores a rejected draft and retries through the same host outbox", async ({ page }) => {
  await page.addInitScript(() => {
    const NativeWebSocket = window.WebSocket;
    let rejectFirst = true;
    window.WebSocket = class extends NativeWebSocket {
      send(data: string): void {
        const command = JSON.parse(data);
        if (command.type === "send_message" && rejectFirst) {
          rejectFirst = false;
          window.setTimeout(() => this.dispatchEvent(new MessageEvent("message", { data: JSON.stringify({
            protocol: "sproyt.chat.v1", type: "error", request_id: command.request_id,
            payload: { code: "forbidden", message: "Prøva avviste sendinga" }
          }) })), 20);
          return;
        }
        super.send(data);
      }
    };
  });
  await page.goto("/?participant=playwright-preview-reject&ui=react", { waitUntil: "domcontentloaded" });
  const preview = page.locator("#sproyt-react-preview");
  const input = preview.getByRole("textbox", { name: "Skriv melding" });
  await expect(input).toBeEnabled({ timeout: 15_000 });
  const message = `behald avvist utkast ${Date.now()}`;
  await input.fill(message);
  await input.press("Enter");
  await expect(preview).toContainText("Prøva avviste sendinga");
  await expect(input).toHaveValue(message);
  await expect(input).toBeEnabled();
  await input.press("Enter");
  await expect(preview.getByText(message, { exact: true })).toBeVisible();
  await expect(input).toHaveValue("");
});

test("preview edits and deletes an own message without leaving the shared runtime", async ({ page }) => {
  const commands: string[] = [];
  page.on("websocket", socket => socket.on("framesent", ({ payload }) => {
    const command = JSON.parse(String(payload));
    if (command.type === "edit_message" || command.type === "delete_message") commands.push(command.type);
  }));
  await page.goto("/?participant=playwright-preview-mutations&ui=react", { waitUntil: "domcontentloaded" });
  const preview = page.locator("#sproyt-react-preview");
  const input = preview.getByRole("textbox", { name: "Skriv melding" });
  await expect(input).toBeEnabled({ timeout: 15_000 });
  const original = `endre meg ${Date.now()}`;
  const edited = `${original} redigert`;
  await input.fill(original);
  await input.press("Enter");
  const card = preview.locator("[data-message-id]").filter({ hasText: original });
  const messageId = await card.getAttribute("data-message-id");
  await card.getByRole("button", { name: "Fleire meldingsval" }).click();
  await card.getByRole("button", { name: "Rediger", exact: true }).click();
  const editor = preview.getByRole("dialog", { name: "Rediger melding" });
  await editor.getByRole("textbox", { name: "Melding" }).fill(edited);
  await editor.getByRole("button", { name: "Lagre", exact: true }).click();
  const editedCard = preview.locator(`[data-message-id="${messageId}"]`);
  await expect(editedCard).toContainText(edited);
  await editedCard.getByRole("button", { name: "Slett", exact: true }).click();
  const confirm = preview.getByRole("dialog", { name: "Slett melding" });
  await confirm.getByRole("button", { name: "Slett melding", exact: true }).click();
  await expect(editedCard).toContainText("Sletta");
  expect(commands).toEqual(["edit_message", "delete_message"]);
});

for (const draft of ["/imagegen"]) {
  test(`preview rejects incomplete image command without sending: ${draft}`, async ({ page }) => {
    let sends = 0;
    page.on("websocket", socket => socket.on("framesent", ({ payload }) => {
      if (JSON.parse(String(payload)).type === "send_message") sends++;
    }));
    await page.goto("/?participant=playwright-preview-handoff&ui=react", { waitUntil: "domcontentloaded" });
    const preview = page.locator("#sproyt-react-preview");
    const input = preview.getByRole("textbox", { name: "Skriv melding" });
    await expect(input).toBeEnabled({ timeout: 15_000 });
    await input.fill(draft);
    await input.press("Enter");
    await expect(preview).toBeVisible();
    await expect(input).toHaveValue(draft);
    await expect(preview).toContainText("1–2000 teikn");
    expect(sends).toBe(0);
  });
}

test("preview image workshop opens with the draft intact", async ({ page }) => {
  await page.goto("/?participant=playwright-preview-tools&ui=react", { waitUntil: "domcontentloaded" });
  const preview = page.locator("#sproyt-react-preview");
  const input = preview.getByRole("textbox", { name: "Skriv melding" });
  await expect(input).toBeEnabled({ timeout: 15_000 });
  await input.fill("vedlegg kjem her");
  await preview.getByRole("button", { name: "Skriveverktøy", exact: true }).click();
  await preview.getByRole("button", { name: "Biletegenerering", exact: true }).click();
  await expect(preview.getByRole("region", { name: "Private biletmeldingar" })).toBeVisible();
  await expect(input).toHaveValue("vedlegg kjem her");
});

test("preview threads keep root and reply drafts separate, load replies, and restore focus", async ({ page }) => {
  let sockets = 0;
  const sends: { body: string; parent_message_id?: string }[] = [];
  page.on("websocket", socket => {
    sockets++;
    socket.on("framesent", ({ payload }) => {
      const command = JSON.parse(String(payload));
      if (command.type === "send_message") sends.push(command.payload);
    });
  });
  await page.goto("/?participant=playwright-preview-threads&ui=react", { waitUntil: "domcontentloaded" });
  const preview = page.locator("#sproyt-react-preview");
  const channel = preview.locator(".sp-channel-pane");
  const composer = channel.getByRole("textbox", { name: "Skriv melding" });
  await expect(composer).toBeEnabled({ timeout: 15_000 });
  const root = `trådrot ${Date.now()}`;
  await composer.fill(root);
  await composer.press("Enter");
  const rootMessage = channel.locator("[data-message-id]").filter({ hasText: root });
  await expect(rootMessage).toBeVisible();
  const rootId = await rootMessage.getAttribute("data-message-id");
  await composer.fill("separat kanalutkast");
  const trigger = rootMessage.getByRole("button", { name: "Svar i tråd" });
  await trigger.click();
  const thread = preview.locator(".sp-thread-pane");
  const reply = thread.getByRole("textbox", { name: "Svar i tråden" });
  await expect(thread).toBeVisible();
  await expect(reply).toBeFocused();
  expect(await page.locator("#thread-panel").evaluate(element => element.matches(":modal"))).toBe(false);
  await reply.fill("separat trådutkast");
  await reply.press("Escape");
  await expect(thread).toHaveCount(0);
  await expect(trigger).toBeFocused();
  await expect(composer).toHaveValue("separat kanalutkast");
  await trigger.click();
  await expect(reply).toHaveValue("separat trådutkast");
  const answer = `trådsvar ${Date.now()}`;
  await reply.fill(answer);
  await reply.press("Enter");
  await expect(reply).toHaveValue("");
  await expect(reply).toBeFocused();
  await expect(thread.getByText(answer, { exact: true })).toBeVisible();
  await expect(channel.getByText(answer, { exact: true })).toHaveCount(0);
  await thread.getByRole("button", { name: "Lukk tråden" }).click();
  await expect(rootMessage.getByRole("button", { name: "1 svar" })).toBeFocused();
  await rootMessage.getByRole("button", { name: "1 svar" }).click();
  await expect(thread.getByText(answer, { exact: true })).toBeVisible();
  expect(sends.filter(command => command.body === answer)).toEqual([expect.objectContaining({ parent_message_id: rootId, body: answer })]);
  expect(sockets).toBe(1);

  // The channel remains mounted with its draft/reading position when the
  // container switches from split panes to the compact thread detail.
  await page.setViewportSize({ width: 1400, height: 900 });
  await expect(channel).toBeVisible();
  await page.setViewportSize({ width: 600, height: 850 });
  await expect(channel).toBeHidden();
  await expect(thread).toBeVisible();
  await expect(reply).toBeVisible();
  await thread.getByRole("button", { name: "Lukk tråden" }).click();
  await expect(channel).toBeVisible();
  await expect(composer).toHaveValue("separat kanalutkast");
});

test("preview retries a failed thread load and a rejected reply without losing its draft", async ({ page }) => {
  await page.addInitScript(() => {
    const NativeWebSocket = window.WebSocket;
    let rejectLoad = true;
    let rejectReply = true;
    window.WebSocket = class extends NativeWebSocket {
      send(data: string): void {
        const command = JSON.parse(data);
        const failLoad = command.type === "load_thread" && rejectLoad;
        const failReply = command.type === "send_message" && command.payload.parent_message_id && rejectReply;
        if (failLoad || failReply) {
          if (failLoad) rejectLoad = false;
          if (failReply) rejectReply = false;
          window.setTimeout(() => this.dispatchEvent(new MessageEvent("message", { data: JSON.stringify({
            protocol: "sproyt.chat.v1", type: "error", request_id: command.request_id,
            payload: { code: "forbidden", message: failLoad ? "Trådlasting avvist i prøva" : "Trådsvar avvist i prøva" }
          }) })), 20);
          return;
        }
        super.send(data);
      }
    };
  });
  await page.goto("/?participant=playwright-preview-thread-retry&ui=react", { waitUntil: "domcontentloaded" });
  const preview = page.locator("#sproyt-react-preview");
  const composer = preview.getByRole("textbox", { name: "Skriv melding" });
  await expect(composer).toBeEnabled({ timeout: 15_000 });
  const root = `prøverot ${Date.now()}`;
  await composer.fill(root);
  await composer.press("Enter");
  await preview.locator("[data-message-id]").filter({ hasText: root }).getByRole("button", { name: "Svar i tråd" }).click();
  const thread = preview.locator(".sp-thread-pane");
  const reply = thread.getByRole("textbox", { name: "Svar i tråden" });
  await expect(thread).toContainText("Trådlasting avvist i prøva");
  await reply.fill("dette svaret skal bevarast");
  await thread.getByRole("button", { name: "Prøv igjen" }).click();
  await expect(thread.getByText("Trådlasting avvist i prøva")).toHaveCount(0);
  await expect(reply).toHaveValue("dette svaret skal bevarast");
  await reply.press("Enter");
  await expect(thread).toContainText("Trådsvar avvist i prøva");
  await expect(reply).toHaveValue("dette svaret skal bevarast");
  await expect(reply).toBeEnabled();
  await reply.press("Enter");
  await expect(thread.getByText("dette svaret skal bevarast", { exact: true })).toBeVisible();
  await expect(reply).toHaveValue("");
});

for (const draft of ["/imagegen", "vedlegg i tråden"]) {
  test(`preview image tool keeps the thread draft isolated: ${draft}`, async ({ page }) => {
    let replies = 0;
    page.on("websocket", socket => socket.on("framesent", ({ payload }) => {
      const command = JSON.parse(String(payload));
      if (command.type === "send_message" && command.payload.parent_message_id) replies++;
    }));
    await page.goto("/?participant=playwright-preview-thread-handoff&ui=react", { waitUntil: "domcontentloaded" });
    const preview = page.locator("#sproyt-react-preview");
    const composer = preview.getByRole("textbox", { name: "Skriv melding" });
    await expect(composer).toBeEnabled({ timeout: 15_000 });
    const root = `handoffrot ${Date.now()}`;
    await composer.fill(root);
    await composer.press("Enter");
    await expect(preview.getByText(root, { exact: true })).toBeVisible();
    await composer.fill("kanalutkast ved handoff");
    await preview.locator("[data-message-id]").filter({ hasText: root }).getByRole("button", { name: "Svar i tråd" }).click();
    const thread = preview.locator(".sp-thread-pane");
    const reply = thread.getByRole("textbox", { name: "Svar i tråden" });
    await reply.fill(draft);
    if (draft.startsWith("vedlegg")) {
      await thread.getByRole("button", { name: "Skriveverktøy", exact: true }).click();
      await thread.getByRole("button", { name: "Biletegenerering", exact: true }).click();
    }
    else await reply.press("Enter");
    await expect(preview).toBeVisible();
    await expect(reply).toHaveValue(draft);
    await expect(composer).toHaveValue("kanalutkast ved handoff");
    if (draft === "/imagegen") await expect(thread).toContainText("1–2000 teikn");
    else await expect(thread.getByRole("region", { name: "Private biletmeldingar" })).toBeVisible();
    expect(replies).toBe(0);
  });
}

test("preview thread switching keeps each draft and full interface handoff preserves them", async ({ page }) => {
  await page.setViewportSize({ width: 1400, height: 900 });
  await page.goto("/?participant=playwright-preview-thread-switch&ui=react", { waitUntil: "domcontentloaded" });
  const preview = page.locator("#sproyt-react-preview");
  const channel = preview.locator(".sp-channel-pane");
  const composer = channel.getByRole("textbox", { name: "Skriv melding" });
  await expect(composer).toBeEnabled({ timeout: 15_000 });
  const roots = [`fyrste rot ${Date.now()}`, `andre rot ${Date.now()}`];
  for (const root of roots) {
    await composer.fill(root);
    await composer.press("Enter");
    await expect(channel.getByText(root, { exact: true })).toBeVisible();
    await expect(composer).toBeEnabled();
  }
  const trigger = (index: number) => channel.locator("[data-message-id]").filter({ hasText: roots[index] }).getByRole("button", { name: "Svar i tråd" });
  const reply = preview.locator(".sp-thread-pane").getByRole("textbox", { name: "Svar i tråden" });
  await trigger(0).click();
  await reply.fill("utkast til fyrste tråd");
  await trigger(1).click();
  await expect(reply).toHaveValue("");
  await reply.fill("utkast til andre tråd");
  await trigger(0).click();
  await expect(reply).toHaveValue("utkast til fyrste tråd");
  await trigger(1).click();
  await expect(reply).toHaveValue("utkast til andre tråd");
  await composer.fill("hovudutkast");
  await preview.getByRole("button", { name: "Meny", exact: true }).click();
  await preview.getByRole("button", { name: "Til fullt grensesnitt", exact: true }).click();
  await expect(preview).toHaveCount(0);
  await expect(page.locator("#thread-panel")).toBeVisible();
  await expect(page.locator("#body")).toHaveValue("hovudutkast");
  await expect(page.locator("#thread-body")).toBeFocused();
  await expect(page.locator("#thread-body")).toHaveValue("utkast til andre tråd");
});
