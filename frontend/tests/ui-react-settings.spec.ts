import { expect, test, type Page } from "@playwright/test";

async function settings(page: Page, label: "Profil og status" | "Varslingsinnstillingar") {
  const preview = page.locator("#sproyt-react-preview");
  await preview.getByRole("button", { name: "Meny", exact: true }).click();
  await preview.getByRole("button", { name: "Meny og innstillingar", exact: true }).click();
  await preview.getByRole("button", { name: label, exact: true }).click();
  return preview.getByRole("dialog", { name: label, exact: true });
}

test("profile saves name and status, clears status and retains the chat draft without reconnecting", async ({ page }) => {
  let sockets = 0;
  page.on("websocket", () => sockets++);
  await page.goto("/?participant=preview-profile-save&ui=react");
  const preview = page.locator("#sproyt-react-preview");
  const composer = preview.getByRole("textbox", { name: "Skriv melding" });
  await expect(composer).toBeEnabled({ timeout: 15000 });
  await composer.fill("utkast medan profilen endrast");
  const dialog = await settings(page, "Profil og status");
  await dialog.getByRole("textbox", { name: "Visningsnamn", exact: true }).fill("Nytt profilnamn");
  await dialog.getByRole("button", { name: "Lagre namn", exact: true }).click();
  await expect(dialog).toContainText("Namnet er lagra.");
  await dialog.getByRole("textbox", { name: "Statusemoji", exact: true }).fill("🌻");
  await dialog.getByRole("textbox", { name: "Statusmelding", exact: true }).fill("Ute i sola");
  await dialog.getByRole("button", { name: "Lagre status", exact: true }).click();
  await expect(dialog).toContainText("Statusen er lagra.");
  await page.keyboard.press("Escape");
  await preview.getByRole("button", { name: "Profil og status", exact: true }).click();
  await expect(dialog.getByLabel("Visningsnamn", { exact: true })).toHaveValue("Nytt profilnamn");
  await expect(dialog.getByLabel("Statusmelding", { exact: true })).toHaveValue("Ute i sola");
  await dialog.getByRole("button", { name: "Tøm status", exact: true }).click();
  await expect(dialog).toContainText("Statusen er tømd.");
  await expect(dialog.getByLabel("Statusemoji", { exact: true })).toHaveValue("");
  await page.keyboard.press("Escape"); await page.keyboard.press("Escape");
  await expect(composer).toHaveValue("utkast medan profilen endrast");
  expect(sockets).toBe(1);
  expect(new URL(page.url()).searchParams.get("ui")).toBe("react");
});

test("profile server rejection keeps the entered name and allows retry", async ({ page }) => {
  await page.addInitScript(() => {
    const NativeWebSocket = window.WebSocket;
    let first = true;
    window.WebSocket = class extends NativeWebSocket {
      send(data: string) {
        const command = JSON.parse(data);
        if (command.type === "update_profile" && first) {
          first = false;
          setTimeout(() => this.dispatchEvent(new MessageEvent("message", { data: JSON.stringify({
            protocol: "sproyt.chat.v1", type: "error", request_id: command.request_id,
            payload: { code: "validation_error", message: "Profilen vart avvist" }
          }) })), 50);
        } else super.send(data);
      }
    };
  });
  await page.goto("/?participant=preview-profile-retry&ui=react");
  await expect(page.locator("#sproyt-react-preview").getByRole("textbox", { name: "Skriv melding" })).toBeEnabled();
  const dialog = await settings(page, "Profil og status");
  await dialog.getByLabel("Visningsnamn", { exact: true }).fill("Prøv dette namnet");
  await dialog.getByRole("button", { name: "Lagre namn", exact: true }).click();
  await expect(dialog).toContainText("Profilen vart avvist");
  await expect(dialog.getByLabel("Visningsnamn", { exact: true })).toHaveValue("Prøv dette namnet");
  await dialog.getByRole("button", { name: "Lagre namn", exact: true }).click();
  await expect(dialog).toContainText("Namnet er lagra.");
});

test("notifications preserve failed edits, retry and persist all preferences", async ({ page }) => {
  await page.goto("/?participant=preview-notification-settings&ui=react");
  await expect(page.locator("#sproyt-react-preview").getByRole("textbox", { name: "Skriv melding" })).toBeEnabled();
  let fail = true;
  await page.route("**/api/v1/me/notifications?*", route => {
    if (route.request().method() === "PUT" && fail) {
      fail = false;
      return route.fulfill({ status: 503, body: "Mellombels varslingsfeil" });
    }
    return route.continue();
  });
  const dialog = await settings(page, "Varslingsinnstillingar");
  await dialog.getByLabel("Varslingsmodus", { exact: true }).selectOption("weekly");
  await dialog.getByLabel("Direktemeldingar", { exact: true }).uncheck();
  await dialog.getByLabel("Omtalar", { exact: true }).uncheck();
  await dialog.getByRole("button", { name: "Lagre varslingsinnstillingar", exact: true }).click();
  await expect(dialog).toContainText("Mellombels varslingsfeil");
  await expect(dialog.getByLabel("Varslingsmodus", { exact: true })).toHaveValue("weekly");
  await dialog.getByRole("button", { name: "Lagre varslingsinnstillingar", exact: true }).click();
  await expect(dialog).toContainText("Varslingsinnstillingane er lagra.");
  await page.keyboard.press("Escape");
  await page.locator("#sproyt-react-preview").getByRole("button", { name: "Varslingsinnstillingar", exact: true }).click();
  await expect(dialog.getByLabel("Varslingsmodus", { exact: true })).toHaveValue("weekly");
  await expect(dialog.getByLabel("Direktemeldingar", { exact: true })).not.toBeChecked();
  await expect(dialog.getByLabel("Omtalar", { exact: true })).not.toBeChecked();
  await expect(dialog).toContainText("Push er ikkje konfigurert på serveren enno.");
  await expect(dialog.getByRole("button", { name: "Slå på varsel på denne eininga", exact: true })).toBeDisabled();
});

test("raw text toggle retains markdown source and returns to safe formatted content", async ({ page }) => {
  await page.goto("/?participant=preview-raw-content&ui=react");
  const preview = page.locator("#sproyt-react-preview");
  const input = preview.getByRole("textbox", { name: "Skriv melding" });
  await expect(input).toBeEnabled();
  const source = `**råtekst ${Date.now()}** <script>window.unsafe=true</script>`;
  await input.fill(source); await input.press("Enter");
  const message = preview.locator("[data-message-id]").filter({ hasText: /råtekst/ }).last();
  await expect(message.locator(".sp-message-content strong")).toHaveText(/råtekst/);
  await preview.getByRole("button", { name: "Meny", exact: true }).click();
  await preview.getByRole("button", { name: "Vis råtekst", exact: true }).click();
  await expect(message.locator("pre")).toHaveText(source);
  await expect(message.locator("script")).toHaveCount(0);
  await preview.getByRole("button", { name: "Vis formatert", exact: true }).click();
  await expect(message.locator(".sp-message-content strong")).toHaveText(/råtekst/);
});

test("notification load can be retried and push uses the host registration after explicit consent", async ({ page }) => {
  await page.addInitScript(() => {
    const subscription = { toJSON: () => ({ endpoint: "https://push.example.test/device", keys: { p256dh: "key", auth: "auth" } }) };
    const registration = { pushManager: { getSubscription: async () => subscription } };
    Object.defineProperty(navigator, "serviceWorker", { value: {
      register: async () => registration, ready: Promise.resolve(registration)
    } });
    Object.defineProperty(Notification, "permission", { configurable: true, get: () => "default" });
    Notification.requestPermission = async () => {
      document.documentElement.dataset.pushConsent = "requested";
      Object.defineProperty(Notification, "permission", { configurable: true, get: () => "granted" });
      return "granted";
    };
  });
  let failure = false;
  let registrations = 0;
  await page.route("**/api/v1/me/notifications?*", route => {
    if (failure) { failure = false; return route.fulfill({ status: 503, body: "Varslingsoppsettet kunne ikkje lastast" }); }
    return route.fulfill({ json: {
      enabled: true, subscriptions: registrations, public_key: "test", channel_ids: [],
      preferences: { mode: "instant", direct_messages: true, mentions: true }
    } });
  });
  await page.route("**/api/v1/me/push-subscriptions?*", route => {
    registrations++;
    expect(route.request().postDataJSON().endpoint).toBe("https://push.example.test/device");
    return route.fulfill({ status: 204 });
  });
  await page.goto("/?participant=preview-push-registration&ui=react");
  await expect(page.locator("#sproyt-react-preview").getByRole("textbox", { name: "Skriv melding" })).toBeEnabled();
  // The host loads once at startup; fail only the settings dialog's read.
  await expect(page.locator("#notification-notice")).toContainText("Varsel er ikkje slått på");
  failure = true;
  const dialog = await settings(page, "Varslingsinnstillingar");
  await expect(dialog).toContainText("Varslingsoppsettet kunne ikkje lastast");
  await dialog.getByRole("button", { name: "Prøv igjen", exact: true }).click();
  await expect(dialog.getByLabel("Varslingsmodus", { exact: true })).toBeVisible();
  expect(registrations).toBe(0);
  await dialog.getByRole("button", { name: "Slå på varsel på denne eininga", exact: true }).click();
  await expect(dialog).toContainText("Varsel er slått på på denne eininga.");
  await expect(dialog).toContainText("Nettlesaren har tillate varsel.");
  expect(registrations).toBe(1);
  await expect(page.locator("html")).toHaveAttribute("data-push-consent", "requested");
});
