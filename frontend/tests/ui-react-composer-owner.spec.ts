import { expect, test, type Page } from "@playwright/test";

async function poisonLegacyComposer(page: Page) {
  await page.evaluate(() => {
    for (const id of ["body", "thread-body"]) {
      const field = document.getElementById(id)!;
      Object.defineProperty(field, "value", {
        get() { throw new Error(`React read legacy ${id}.value`); },
        set() { throw new Error(`React wrote legacy ${id}.value`); }
      });
      for (const property of ["disabled", "readOnly"]) {
        Object.defineProperty(field, property, {
          get() { throw new Error(`React read legacy ${id}.${property}`); },
          set() { /* Legacy status may still mirror transport state. */ }
        });
      }
      field.dispatchEvent = () => { throw new Error(`React dispatched an event to legacy ${id}`); };
      field.closest("form")!.requestSubmit = () => { throw new Error(`React submitted legacy ${id} form`); };
    }
    const panel = document.getElementById("thread-panel") as HTMLDialogElement;
    panel.show = () => { throw new Error("React opened the legacy thread panel"); };
    panel.showModal = () => { throw new Error("React opened the legacy thread modal"); };
    panel.close = () => { throw new Error("React closed the legacy thread panel"); };
  });
}

test("React draft state and sends survive poisoned hidden controls, channel changes, thread changes and reload", async ({ page }) => {
  await page.setViewportSize({ width: 1400, height: 900 });
  const errors: string[] = [];
  const sent: string[] = [];
  page.on("pageerror", error => errors.push(error.message));
  page.on("websocket", socket => socket.on("framesent", ({ payload }) => {
    const command = JSON.parse(String(payload));
    if (command.type === "send_message") sent.push(command.payload.body);
  }));
  await page.goto(`/?participant=composer-owner-${Date.now()}&ui=react`);
  const preview = page.locator("#sproyt-react-preview");
  const composer = preview.getByRole("textbox", { name: "Skriv melding" });
  await expect(composer).toBeEnabled({ timeout: 15_000 });
  await poisonLegacyComposer(page);
  const rootBody = `Modellstyrt rot ${Date.now()}`;
  await composer.fill(rootBody);
  await composer.press("Enter");
  const root = preview.locator(".sp-channel-pane [data-message-id]").filter({ hasText: rootBody });
  await expect(root).toBeVisible();
  await expect(composer).toHaveValue("");
  await composer.fill("kanalutkast utan skjult felt");

  if (!await preview.getByRole("button", { name: "Meny og innstillingar", exact: true }).isVisible()) {
    await preview.getByRole("button", { name: "Meny", exact: true }).click();
  }
  await preview.getByRole("button", { name: "Meny og innstillingar", exact: true }).click();
  await preview.getByRole("dialog", { name: "Meny og innstillingar" }).getByRole("button", { name: "Kanalar i Felles", exact: true }).click();
  const channels = preview.getByRole("dialog", { name: "Kanalar i Felles" });
  const channelName = `Modellkanal ${Date.now()}`;
  await channels.getByLabel("Kanalnamn").fill(channelName);
  await channels.getByLabel("Kanaltype").selectOption("public");
  await channels.getByRole("button", { name: "Opprett kanal" }).click();
  await expect(channels).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(composer).toHaveValue("");
  await composer.fill("anna kanalutkast");
  await preview.getByRole("button", { name: /^# general/i }).click();
  await expect(composer).toHaveValue("kanalutkast utan skjult felt");
  await root.getByRole("button", { name: "Svar i tråd" }).click();
  const reply = preview.locator(".sp-thread-pane").getByRole("textbox", { name: "Svar i tråden" });
  await reply.fill("trådutkast utan skjult felt");
  await page.keyboard.press("Escape");
  await root.getByRole("button", { name: "Svar i tråd" }).click();
  await expect(reply).toHaveValue("trådutkast utan skjult felt");
  await page.reload();
  await expect(composer).toBeEnabled({ timeout: 15_000 });
  await poisonLegacyComposer(page);
  await expect(composer).toHaveValue("kanalutkast utan skjult felt");
  await root.getByRole("button", { name: "Svar i tråd" }).click();
  await expect(reply).toHaveValue("trådutkast utan skjult felt");
  await reply.press("Enter");
  await expect(preview.locator(".sp-thread-pane").getByText("trådutkast utan skjult felt", { exact: true })).toBeVisible();
  await expect(reply).toHaveValue("");
  await page.keyboard.press("Escape");
  await composer.press("Enter");
  await expect(preview.locator(".sp-channel-pane").getByText("kanalutkast utan skjult felt", { exact: true })).toBeVisible();
  await preview.getByRole("button", { name: `# ${channelName}`, exact: true }).click();
  await expect(composer).toHaveValue("anna kanalutkast");
  expect(sent).toEqual([rootBody, "trådutkast utan skjult felt", "kanalutkast utan skjult felt"]);
  expect(errors).toEqual([]);
});
