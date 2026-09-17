import { expect, test, type Locator, type Page } from "@playwright/test";

async function enter(page: Page, enabled = true) {
  // Expose the server-rendered feature flags for this fixture, preserving real
  // application capabilities and real user/channel memberships underneath.
  if (enabled) await page.route(/\/\?participant=/, async route => {
    const response = await route.fetch();
    await route.fulfill({ response, body: (await response.text()).replace('class="agent-access" hidden', 'class="agent-access"').replace('class="advanced-tools" hidden', 'class="advanced-tools"') });
  });
  await page.goto(`/?participant=advanced-${Date.now()}&ui=react`);
  const preview = page.locator("#sproyt-react-preview");
  await expect(preview.getByRole("textbox", { name: "Skriv melding" })).toBeEnabled();
  return preview;
}
async function menu(preview: Locator, label: string) {
  const menu = preview.getByRole("dialog", { name: "Meny og innstillingar", exact: true });
  if (!await menu.isVisible()) {
    const trigger = preview.getByRole("button", { name: "Meny og innstillingar", exact: true });
    if (!await trigger.isVisible()) await preview.getByRole("button", { name: "Meny", exact: true }).click();
    await trigger.click();
  }
  await menu.getByRole("button", { name: label, exact: true }).click();
}
async function owner(preview: Locator) {
  await menu(preview, "Ny vennekrets");
  const dialog = preview.getByRole("dialog", { name: "Ny vennekrets", exact: true });
  await dialog.getByLabel("Namn på vennekrets").fill(`Avanserte funksjonar ${Date.now()}`);
  await dialog.getByRole("button", { name: "Opprett vennekrets" }).click();
  await expect(dialog).toHaveCount(0);
  await expect(preview.getByRole("button", { name: "# Prat", exact: true })).toBeVisible();
}

test("feature flags hide agent/Heart and member role hides Grafana", async ({ page }) => {
  const preview = await enter(page, false);
  await preview.getByRole("button", { name: "Meny", exact: true }).click();
  await preview.getByRole("button", { name: "Meny og innstillingar", exact: true }).click();
  await expect(preview.getByRole("button", { name: "Agenttilgang", exact: true })).toHaveCount(0);
  await expect(preview.getByRole("button", { name: "Heart og planlegging", exact: true })).toHaveCount(0);
  await menu(preview, "Kanaldetaljar, medlemmer og integrasjonar");
  await expect(preview.getByRole("button", { name: "Lag Grafana-nøkkel" })).toHaveCount(0);
});

test("real short-lived agent grants both scopes, clears secret on close and retains revoke without reconnect", async ({ page }) => {
  let sockets = 0;
  const grants: any[] = [];
  page.on("websocket", () => sockets++);
  page.on("request", request => { if (request.url().includes("/grants")) grants.push(request.postDataJSON()); });
  const preview = await enter(page);
  await menu(preview, "Agenttilgang");
  const agent = preview.getByRole("dialog", { name: "Agenttilgang", exact: true });
  await expect(agent.getByRole("button", { name: "Lag kortliva tilgang" })).toBeDisabled();
  await page.keyboard.press("Escape");
  await owner(preview);
  await page.keyboard.press("Escape");
  await preview.getByRole("textbox", { name: "Skriv melding" }).fill("agentutkast");
  await menu(preview, "Agenttilgang");
  await agent.getByRole("button", { name: "Lag kortliva tilgang" }).click();
  await expect(agent.getByLabel("Agentcredential", { exact: true })).not.toHaveValue("");
  const credential = await agent.getByLabel("Agentcredential", { exact: true }).inputValue();
  expect(grants.map(item => item.scope)).toEqual(["read_history", "send_messages"]);
  expect(grants[0].channel_id).toBe(grants[1].channel_id);
  expect(await page.evaluate(() => JSON.stringify({ local: localStorage, session: sessionStorage }))).not.toContain(credential);
  await page.keyboard.press("Escape");
  await menu(preview, "Agenttilgang");
  await expect(agent.getByLabel("Agentcredential", { exact: true })).toHaveCount(0);
  await agent.getByRole("button", { name: "Trekk tilbake", exact: true }).click();
  await expect(agent).toContainText("Agenttilgangen er trekt tilbake.");
  await expect(agent.getByRole("button", { name: "Lag kortliva tilgang" })).toBeEnabled();
  await page.keyboard.press("Escape"); await page.keyboard.press("Escape");
  await expect(preview.getByRole("textbox", { name: "Skriv melding" })).toHaveValue("agentutkast");
  expect(sockets).toBe(1);
});

test("failed agent grant rolls back and failed rollback keeps explicit revoke and retry", async ({ page }) => {
  let failRevoke = true;
  let revokes = 0;
  await page.route("**/api/v1/agents/*/grants?*", route => route.fulfill({ status: 503, body: "Kanalrettar feila" }));
  await page.route("**/api/v1/agents/*/revoke?*", route => {
    revokes++;
    if (failRevoke) { failRevoke = false; return route.fulfill({ status: 503, body: "Tilbakekalling feila" }); }
    return route.continue();
  });
  const preview = await enter(page); await owner(preview); await menu(preview, "Agenttilgang");
  const agent = preview.getByRole("dialog", { name: "Agenttilgang", exact: true });
  await agent.getByRole("button", { name: "Lag kortliva tilgang" }).click();
  await expect(agent).toContainText("tilbakekalling feila");
  await expect(agent.getByLabel("Agentcredential", { exact: true })).toHaveCount(0);
  await agent.getByRole("button", { name: "Trekk tilbake", exact: true }).click();
  await expect(agent).toContainText("Agenttilgangen er trekt tilbake.");
  expect(revokes).toBe(2);
});

test("Grafana creation retries, exposes token once and clears it on details close", async ({ page, context }) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  let first = true;
  await page.route("**/integrations/grafana?*", route => {
    if (first) { first = false; return route.fulfill({ status: 503, body: "Grafana er mellombels utilgjengeleg" }); }
    return route.continue();
  });
  const preview = await enter(page); await owner(preview);
  await menu(preview, "Kanaldetaljar, medlemmer og integrasjonar");
  const dialog = preview.getByRole("dialog", { name: "Kanaldetaljar: Prat", exact: true });
  await dialog.getByRole("button", { name: "Lag Grafana-nøkkel" }).click();
  await expect(dialog).toContainText("Grafana er mellombels utilgjengeleg");
  await dialog.getByRole("button", { name: "Lag Grafana-nøkkel" }).click();
  await expect(dialog.getByLabel("Grafana-token", { exact: true })).not.toHaveValue("");
  const token = await dialog.getByLabel("Grafana-token", { exact: true }).inputValue();
  await dialog.getByRole("button", { name: "Kopier grafana-token" }).click();
  await expect(dialog).toContainText("Grafana-token er kopiert.");
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(token);
  await page.keyboard.press("Escape");
  await menu(preview, "Kanaldetaljar, medlemmer og integrasjonar");
  await expect(dialog.getByLabel("Grafana-token", { exact: true })).toHaveCount(0);
  expect(await page.evaluate(() => JSON.stringify({ local: localStorage, session: sessionStorage }))).not.toContain(token);
});

test("Heart enables feature, starts scoped process, retries status and sends inspect and yes/no", async ({ page }) => {
  const requests: Array<{ url: string; body: any }> = [];
  let failedStatus = false;
  await page.route(/\/api\/v1\/(processes|circles\/[^/]+\/features\/heart-event-planning)/, route => {
    const request = route.request();
    const path = new URL(request.url()).pathname;
    if (request.method() === "POST") requests.push({ url: path, body: request.postDataJSON() });
    if (path === "/api/v1/processes") return route.fulfill({ json: { process_link_id: "fixture-process" } });
    if (request.method() === "GET") {
      if (!failedStatus) { failedStatus = true; return route.fulfill({ status: 503, body: "Prøv status igjen" }); }
      return route.fulfill({ json: { process: { definition_name: "event-planning", status: "waiting" }, events: [{ event_type: "started", actor_id: "Heart", payload: { title: "Felles middag" } }] } });
    }
    return route.fulfill({ status: 204 });
  });
  const preview = await enter(page); await owner(preview); await menu(preview, "Heart og planlegging");
  const dialog = preview.getByRole("dialog", { name: "Heart og planlegging", exact: true });
  await dialog.getByRole("button", { name: "Slå på event-planlegging" }).click();
  await expect(dialog).toContainText("Event-planlegging er slått på");
  await dialog.getByLabel("Tittel på planlegging").fill("Felles middag");
  await dialog.getByRole("button", { name: "Start planlegging", exact: true }).click();
  await expect(dialog).toContainText("Prøv status igjen");
  await expect(dialog.getByLabel("Prosess-ID", { exact: true })).toHaveValue("fixture-process");
  await dialog.getByRole("button", { name: "Oppdater status" }).click();
  await expect(dialog.getByRole("region", { name: "Prosessstatus" })).toContainText("event-planning: waiting");
  await dialog.getByRole("button", { name: "Inspiser Heart" }).click();
  await expect(dialog).toContainText("Heart-status er lagd i den varige køen");
  await dialog.getByRole("button", { name: "Svar ja" }).click();
  await expect(dialog).toContainText("Svaret «ja»");
  await dialog.getByRole("button", { name: "Svar nei" }).click();
  await expect(dialog).toContainText("Svaret «nei»");
  expect(requests.find(item => item.url === "/api/v1/processes")?.body).toMatchObject({ metadata: { title: "Felles middag" }, channel_id: expect.any(String) });
  expect(requests.filter(item => item.url.endsWith("/messages")).map(item => item.body.payload.answer)).toEqual(["yes", "no"]);
  expect(new Set(requests.filter(item => item.body?.request_id).map(item => item.body.request_id)).size).toBe(4);
  await page.keyboard.press("Escape"); await menu(preview, "Heart og planlegging");
  await expect(dialog.getByLabel("Prosess-ID", { exact: true })).toHaveValue("fixture-process");
});
