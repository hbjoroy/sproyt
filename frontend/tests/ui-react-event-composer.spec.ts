import { expect, test, type Locator } from "@playwright/test";
import { eventComposerFixture } from "./support/event-composer-fixture";

// Keep real Enter-to-send coverage on fine pointers. Touch composers reserve
// Enter for a new line, but support the explicit Control+Enter shortcut.
async function send(composer: Locator) {
  const enterSends = await composer.page().evaluate(() => matchMedia("(any-hover: hover) and (any-pointer: fine)").matches);
  await composer.press(enterSends ? "Enter" : "Control+Enter");
}

async function enter(page: Parameters<typeof eventComposerFixture>[0]) {
  await page.setViewportSize({ width: 1440, height: 900 });
  const server = await eventComposerFixture(page);
  const composer = server.app.getByRole("textbox", { name: "Skriv melding", exact: true });
  const context = server.app.getByRole("group", { name: "Arrangement for meldinga", exact: true });
  await expect(composer).toBeEnabled();
  await server.app.getByRole("link", { name: server.getEvent().title, exact: true }).click();
  await server.app.getByRole("button", { name: "Skriv om arrangementet", exact: true }).click();
  await expect(context).toBeVisible();
  return { ...server, composer, context };
}

test("explicit event send preserves aliases and retries once without duplicate references; plain sends stay plain", async ({ page }) => {
  const f = await enter(page);
  await send(f.composer); expect(f.sends).toHaveLength(0); await expect(f.composer).toHaveValue("");
  const body = "Ta med vener #arr:gammal og #arr:anna";
  await f.composer.fill(body); f.rejectNext(); await send(f.composer);
  await expect(f.app).toContainText("Sendinga vart avvist"); await expect(f.composer).toHaveValue(body); await expect(f.composer).toBeEnabled();
  await expect(f.context).toBeVisible(); await send(f.composer);
  await expect.poll(() => f.sends.length).toBe(2); expect(f.sends.map(send => send.body)).toEqual([body, body]);
  await expect(f.composer).toHaveValue(""); await expect(f.composer).toBeEnabled();
  await f.context.getByRole("button", { name: /Fjern/ }).click();
  await f.composer.fill("Vanleg melding"); await send(f.composer);
  await expect.poll(() => f.sends.length).toBe(3); expect(f.sends[2]!.body).toBe("Vanleg melding");
});

test("attachment-only event send preserves attachment and context on rejection and retries without duplicate codes", async ({ page }) => {
  const f = await enter(page);
  const image = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLbtAAAAABJRU5ErkJggg==", "base64");
  await f.app.getByLabel("Vel bilete eller video", { exact: true }).setInputFiles({ name: "foto.png", mimeType: "image/png", buffer: image });
  const attachments = f.app.getByRole("region", { name: "Valde vedlegg" });
  await expect(attachments.getByRole("button", { name: "Fjern foto.png", exact: true })).toBeEnabled();
  f.rejectNext(); await f.app.getByRole("button", { name: "Send vedlegg", exact: true }).click();
  await expect(f.app).toContainText("Sendinga vart avvist"); await expect(f.composer).toBeEnabled();
  await expect(f.context).toBeVisible(); await expect(attachments.getByRole("button", { name: "Fjern foto.png", exact: true })).toBeEnabled();
  await expect(f.app.getByRole("listbox", { name: "Arrangementsforslag", exact: true })).toHaveCount(0);
  await send(f.composer); await expect.poll(() => f.sends.length).toBe(2);
  expect(f.sends[0]!.body).toBe(f.sends[1]!.body); expect(f.sends[1]!.body.match(/#arr:konsert/gu)).toHaveLength(1);
  expect(f.sends[1]!.body).toContain("[[media:00000000-0000-7000-8000-000000009913|image/png|foto.png]]");
});

test("title search switches explicit event context while preserving draft and selected reading filter", async ({ page }) => {
  const f = await enter(page); await f.composer.fill("Utkast før val");
  await f.context.getByRole("button", { name: "Byt arrangement", exact: true }).click();
  const picker = f.app.getByRole("dialog", { name: "Vel arrangement for meldinga", exact: true });
  await picker.getByRole("searchbox", { name: "Søk etter arrangement" }).fill("Samling B");
  await picker.getByRole("button", { name: /Samling B utanfor første side/ }).click();
  await expect(f.context).toContainText("Samling B utanfor første side"); await expect(f.composer).toHaveValue("Utkast før val");
  expect(f.sends).toHaveLength(0); await send(f.composer); await expect.poll(() => f.sends.length).toBe(1);
  expect(f.sends[0]!.body).toBe("Utkast før val\n\n#arr:samling-b");
});

for (const change of ["channel", "disabled", "account", "access"] as const) test(`event composition context clears on ${change} without sending a draft`, async ({ page }) => {
  const f = await enter(page); await f.composer.fill("Privat utkast");
  if (change === "channel") {
    await f.app.getByRole("button", { name: "# Annan kanal", exact: true }).click();
    await expect(f.composer).toBeEnabled(); await expect(f.context).toHaveCount(0);
    await f.app.getByRole("button", { name: "# Arrangementskanalen", exact: true }).click(); await expect(f.composer).toHaveValue("Privat utkast");
  } else if (change === "disabled") {
    f.setEnabled(false); await expect(f.app.getByRole("tablist", { name: "Innhald i kanalen" })).toHaveCount(0); await expect(f.composer).toHaveValue("Privat utkast");
  } else if (change === "access") {
    f.revoke(); await page.evaluate(() => document.dispatchEvent(new Event("visibilitychange"))); await expect(f.composer).toHaveValue("Privat utkast");
  } else {
    f.switchAccount(); await expect(f.context).toHaveCount(0);
    const group = f.app.locator('[data-conversation-group="scope:shared"]');
    const disclosure = group.locator('.sp-product-nav-disclosure');
    if (await disclosure.getAttribute('aria-expanded') === 'false') await disclosure.click();
    await f.app.getByRole("button", { name: "# Arrangementskanalen", exact: true }).click(); await expect(f.composer).toBeEnabled(); await expect(f.composer).toHaveValue("");
  }
  await expect(f.context).toHaveCount(0); expect(f.sends).toHaveLength(0);
});

test("removing context after a rejected prepared send restores the original draft before an ordinary retry", async ({ page }) => {
  const f = await enter(page); await f.composer.fill("Berre teksten min"); f.rejectNext(); await send(f.composer);
  await expect(f.app).toContainText("Sendinga vart avvist"); await expect(f.composer).toBeEnabled();
  await expect(f.composer).toHaveValue("Berre teksten min\n\n#arr:konsert");
  await f.context.getByRole("button", { name: /Fjern/ }).click(); await expect(f.composer).toHaveValue("Berre teksten min");
  await send(f.composer); await expect.poll(() => f.sends.length).toBe(2);
  expect(f.sends.map(send => send.body)).toEqual(["Berre teksten min\n\n#arr:konsert", "Berre teksten min"]);
});

test("selected event cannot become a silently ignored fifty-first reference", async ({ page }) => {
  const f = await enter(page);
  const body = Array.from({ length: 50 }, (_, index) => `#arr:event-${index}`).join(" ");
  await f.composer.fill(body); await send(f.composer);
  await expect(f.app.getByRole("alert")).toContainText("50 arrangementsreferansar");
  await expect(f.composer).toHaveValue(body); await expect(f.context).toBeVisible(); expect(f.sends).toHaveLength(0);
});

for (const width of [320, 390]) test(`event context and picker fit ${width}px with a long title and automatic code`, async ({ page }, testInfo) => {
  await page.setViewportSize({ width, height: 900 });
  const title = "Felles samling med eit svært langt og lesbart arrangementsnamn for alle i vennekretsen";
  const code = "e-01234567-89ab-7cde-8f01-234567890abc";
  const f = await eventComposerFixture(page, false, false, true, false, { title, code });
  const composer = f.app.getByRole("textbox", { name: "Skriv melding", exact: true });
  await composer.fill("Utkastet er bevart");
  await f.app.getByRole("link", { name: title, exact: true }).click();
  await f.app.getByRole("button", { name: "Skriv om arrangementet", exact: true }).click();
  const context = f.app.getByRole("group", { name: "Arrangement for meldinga", exact: true });
  await expect(context).toContainText(title); await expect(context).not.toContainText(code);
  await expect(context.locator("strong")).toHaveAttribute("title", `#arr:${code}`);
  await expect(composer).toHaveValue("Utkastet er bevart");
  expect(await context.evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
  expect(await f.app.evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
  await f.app.screenshot({ path: testInfo.outputPath(`event-context-${width}.png`) });
  await context.getByRole("button", { name: "Byt arrangement", exact: true }).click();
  const picker = f.app.getByRole("dialog", { name: "Vel arrangement for meldinga", exact: true });
  const option = picker.getByRole("button", { name: new RegExp(title) });
  await expect(option).toBeVisible(); await expect(option).toHaveAttribute("title", `#arr:${code}`);
  await expect(option).toContainText("Europe/Oslo");
  expect(await picker.evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
  await f.app.screenshot({ path: testInfo.outputPath(`event-picker-${width}.png`) });
});
