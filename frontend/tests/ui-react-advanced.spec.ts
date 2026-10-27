import { destinationSurface } from "./support/destination-surface";
import { expect, test, type Locator, type Page } from "@playwright/test";

async function enter(page: Page, enabled = true) {
  // Expose the server-rendered feature flags for this fixture, preserving real
  // application capabilities and real user/channel memberships underneath.
  if (enabled) await page.route(/\/\?participant=/, async route => {
    const response = await route.fetch();
    await route.fulfill({ response, body: (await response.text()).replace(/(<meta name="sproyt-capabilities" content=")[^"]*(")/, "$1agent heart$2").replace('class="agent-access" hidden', 'class="agent-access"').replace('class="advanced-tools" hidden', 'class="advanced-tools"') });
  });
  await page.goto(`/?participant=advanced-${Date.now()}&ui=react`);
  const preview = page.locator("#sproyt-react-preview");
  await expect(preview.getByRole("textbox", { name: "Skriv melding" })).toBeEnabled();
  return preview;
}
async function closeManagedDialog(page: Page) {
  const preview = page.locator("#sproyt-react-preview");
  const before = page.url();
  const dialogs = preview.locator("[data-workspace-page], dialog[open]");
  const previous = await dialogs.count() ? await dialogs.last().elementHandle() : null;
  // A local popup can sit above a management dialog without changing its URL.
  const routed = !!previous && new URL(before).searchParams.get("view") === "settings"
    && await preview.locator("dialog[open]").count() === 1;
  await page.keyboard.press("Escape");
  if (routed) await expect.poll(() => page.url()).not.toBe(before);
  if (previous) await previous.waitForElementState("hidden");
  if (routed && new URL(page.url()).searchParams.get("view") === "settings") {
    // URL publication precedes React mounting the parent. Wait for both sides
    // of Back before a menu helper decides whether it should open the toolbar.
    await expect(dialogs).toHaveCount(1);
    await expect(dialogs).toBeVisible();
  } else if (routed) await expect(dialogs).toHaveCount(0);
}
async function menu(preview: Locator, label: string) {
  const menu = destinationSurface(preview, "Innstillingar", true);
  for (let depth = 0; depth < 4 && !await menu.isVisible(); depth++) {
    const dialogs = preview.locator("[data-workspace-page], dialog[open]");
    if (!await dialogs.count()) break;
    await expect(dialogs).toHaveCount(1);
    await closeManagedDialog(preview.page());
  }
  if (!await menu.isVisible()) {
    const trigger = preview.getByRole("button", { name: "Innstillingar", exact: true });

    await trigger.click();
  }
  const action = menu.getByRole("button", { name: label, exact: true });
  if (!await action.isVisible()) await menu.getByRole("searchbox", { name: "Søk i innstillingar" }).fill(label);
  await action.click();
}
async function owner(preview: Locator) {
  await menu(preview, "Ny vennekrets");
  const dialog = destinationSurface(preview, "Ny vennekrets", true);
  const name = `Avanserte funksjonar ${Date.now()}`;
  await dialog.getByLabel("Namn på vennekrets").fill(name);
  await dialog.getByRole("button", { name: "Opprett vennekrets" }).click();
  await expect(dialog).toHaveCount(0);
  const settings = destinationSurface(preview, "Innstillingar", true);
  if (await settings.isVisible()) {
    await settings.getByRole("button", { name: "Lukk innstillingane", exact: true }).click();
    await expect(settings).toHaveCount(0);
  }
  const back = preview.getByRole("button", { name: "Samtalar", exact: true });
  if (await back.isVisible()) await back.click();
  const tab = preview.getByRole("tab", { name: /^Kretsar/ });
  if (await tab.getAttribute("aria-selected") !== "true") await tab.click();
  const group = preview.locator("[data-conversation-group]").filter({ has: preview.page().getByText(name, { exact: true }) });
  const disclosure = group.locator(".sp-product-nav-disclosure");
  await expect(group).toHaveCount(1);
  if (await disclosure.getAttribute("aria-expanded") !== "true") await disclosure.click();
  await group.getByRole("button", { name: "# Prat", exact: true }).click();
}

test("feature flags hide agent/Heart and member role hides Grafana", async ({ page }) => {
  const preview = await enter(page, false);

  await preview.getByRole("button", { name: "Innstillingar", exact: true }).click();
  await expect(preview.getByRole("button", { name: "Mellombels tilgang for ekstern agent", exact: true })).toHaveCount(0);
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
  await menu(preview, "Kanaldetaljar, medlemmer og integrasjonar");
  await expect(preview.getByRole("button", { name: "Lag kortliva tilgang" })).toHaveCount(0);
  await closeManagedDialog(page);
  await expect(preview.getByRole("button", { name: "Mellombels tilgang for ekstern agent", exact: true })).toHaveCount(0);
  const agent = destinationSurface(preview, "Agenttilgang", true);
  await owner(preview);
  await closeManagedDialog(page);
  await preview.getByRole("textbox", { name: "Skriv melding" }).fill("agentutkast");
  await menu(preview, "Mellombels tilgang for ekstern agent");
  await agent.getByRole("button", { name: "Lag kortliva tilgang" }).click();
  await expect(agent.getByLabel("Agentcredential", { exact: true })).not.toHaveValue("");
  const credential = await agent.getByLabel("Agentcredential", { exact: true }).inputValue();
  expect(grants.map(item => item.scope)).toEqual(["read_history", "send_messages"]);
  expect(grants[0].channel_id).toBe(grants[1].channel_id);
  expect(await page.evaluate(() => JSON.stringify({ local: localStorage, session: sessionStorage }))).not.toContain(credential);
  await closeManagedDialog(page);
  await menu(preview, "Mellombels tilgang for ekstern agent");
  await expect(agent.getByLabel("Agentcredential", { exact: true })).toHaveCount(0);
  await agent.getByRole("button", { name: "Trekk tilbake", exact: true }).click();
  await expect(agent).toContainText("Agenttilgangen er trekt tilbake.");
  await expect(agent.getByRole("button", { name: "Lag kortliva tilgang" })).toBeEnabled();
  await closeManagedDialog(page); await closeManagedDialog(page);
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
  const preview = await enter(page); await owner(preview); await menu(preview, "Mellombels tilgang for ekstern agent");
  const agent = destinationSurface(preview, "Agenttilgang", true);
  await agent.getByRole("button", { name: "Lag kortliva tilgang" }).click();
  await expect(agent).toContainText("tilbakekalling feila");
  await expect(agent.getByLabel("Agentcredential", { exact: true })).toHaveCount(0);
  await agent.getByRole("button", { name: "Trekk tilbake", exact: true }).click();
  await expect(agent).toContainText("Agenttilgangen er trekt tilbake.");
  expect(revokes).toBe(2);
});

test("Grafana creation retries, exposes token once and clears it on details close", async ({ page, context, browserName }) => {
  // WebKit does not implement Playwright's Chromium clipboard permissions.
  if (browserName === "chromium") await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  let first = true;
  await page.route("**/integrations/grafana?*", route => {
    if (first) { first = false; return route.fulfill({ status: 503, body: "Grafana er mellombels utilgjengeleg" }); }
    return route.continue();
  });
  const preview = await enter(page); await owner(preview);
  await menu(preview, "Kanaldetaljar, medlemmer og integrasjonar");
  const dialog = destinationSurface(preview, "Kanaldetaljar: Prat", true);
  await dialog.getByRole("button", { name: "Lag Grafana-nøkkel" }).click();
  await expect(dialog).toContainText("Grafana er mellombels utilgjengeleg");
  await dialog.getByRole("button", { name: "Lag Grafana-nøkkel" }).click();
  await expect(dialog.getByLabel("Grafana-token", { exact: true })).not.toHaveValue("");
  const token = await dialog.getByLabel("Grafana-token", { exact: true }).inputValue();
  await dialog.getByRole("button", { name: "Kopier grafana-token" }).click();
  await expect(dialog).toContainText("Grafana-token er kopiert.");
  if (browserName === "chromium") expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(token);
  await closeManagedDialog(page);
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
  const dialog = destinationSurface(preview, "Heart og planlegging", true);
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
  await closeManagedDialog(page); await menu(preview, "Heart og planlegging");
  await expect(dialog.getByLabel("Prosess-ID", { exact: true })).toHaveValue("fixture-process");
});
