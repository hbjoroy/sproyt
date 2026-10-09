import { expect, test } from "@playwright/test";

test("quiet bells reorder only their scope after success while preserving focus, selection and draft", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const ids = ["00000000-0000-7000-8000-000000000011", "00000000-0000-7000-8000-000000000012"];
  const commands: string[] = [];
  await page.route("**/api/v1/me/notifications**", route => route.fulfill({ json: {
    enabled: true, channel_ids: [ids[1]], preferences: { mode: "instant", direct_messages: true, mentions: true }
  } }));
  let reject = true;
  let hold = false;
  let release = () => {};
  const gate = new Promise<void>(resolve => { release = resolve; });
  await page.route("**/api/v1/channels/*/notifications**", async route => {
    if (reject) { reject = false; return route.fulfill({ status: 503, body: "Try again" }); }
    if (hold) await gate;
    return route.fulfill({ status: 204 });
  });
  await page.routeWebSocket(/\/ws(?:\?|$)/, socket => socket.onMessage(data => {
    const command = JSON.parse(String(data)); commands.push(command.type);
    const reply = (type: string, payload?: unknown) => socket.send(JSON.stringify({ protocol: "sproyt.chat.v1", type, request_id: command.request_id, payload }));
    switch (command.type) {
      case "hello": reply("hello", { participant_id: "quiet-bells" }); break;
      case "ping": reply("pong"); break;
      case "list_users": reply("users_listed", { users: [] }); break;
      case "list_my_circles": reply("circles_listed", { circles: [] }); break;
      case "list_mentions": reply("mentions_listed", { mentions: [] }); break;
      case "list_tasks": reply("tasks_listed", { tasks: [] }); break;
      case "list_my_channels": reply("channels_listed", { channels: ids.map((id, index) => ({ id, slug: `quiet-${index}`, name: index ? "Enabled" : "Muted", kind: "public", circle_id: null, direct_user_id: null, is_direct: false, description: "", role: "member", last_read_sequence: 0, latest_sequence: 0 })) }); break;
      case "subscribe_channel": reply("subscription_started", { channel_id: command.payload.channel_id, history: [] }); break;
      case "list_thread_summaries": reply("thread_summaries_listed", { channel_id: command.payload.channel_id, summaries: [] }); break;
      case "list_channel_reactions": reply("channel_reactions_listed", { channel_id: command.payload.channel_id, reactions: [] }); break;
      case "load_recent_messages": reply("messages_loaded", { channel_id: command.payload.channel_id, messages: [] }); break;
    }
  }));
  await page.goto(`/?participant=quiet-bells&channel=${ids[0]}`);
  const app = page.locator("#sproyt-react-preview");
  const composer = app.getByRole("textbox", { name: "Skriv melding" });
  await expect(composer).toBeEnabled(); await composer.fill("Keep my draft");
  await app.getByRole("button", { name: "Samtalar", exact: true }).click();
  const group = app.locator('[data-conversation-group="scope:shared"]');
  const rows = group.locator(".sp-conversation-row");
  await expect(rows.locator(".sp-conversation-name")).toHaveText(["# Enabled", "# Muted"]);
  const enabled = group.getByRole("button", { name: "Slå av varsel for Enabled" });
  const muted = group.getByRole("button", { name: "Slå på varsel for Muted" });
  for (const colorScheme of ["light", "dark"] as const) {
    await page.emulateMedia({ colorScheme });
    const style = (element: HTMLElement) => { const css = getComputedStyle(element); return { background: css.backgroundColor, shadow: css.boxShadow, color: css.color }; };
    expect(await enabled.evaluate(style)).toEqual(await muted.evaluate(style));
    expect(await enabled.evaluate(element => getComputedStyle(element).boxShadow)).toBe("none");
  }
  expect(await enabled.locator("path").count()).toBe(1); expect(await muted.locator("path").count()).toBe(2);
  const subscriptions = commands.filter(command => command === "subscribe_channel").length;
  await enabled.click();
  await expect(group.getByRole("alert")).toContainText("Try again");
  await expect(rows.locator(".sp-conversation-name")).toHaveText(["# Enabled", "# Muted"]);
  await enabled.click();
  const off = group.getByRole("button", { name: "Slå på varsel for Enabled" });
  await expect(off).toHaveAttribute("aria-pressed", "false"); await expect(off).toBeFocused();
  await expect(rows.locator(".sp-conversation-name")).toHaveText(["# Muted", "# Enabled"]);
  hold = true;
  await off.click(); await expect(off).toBeDisabled();
  const search = app.getByRole("searchbox", { name: "Finn samtale" });
  await search.focus(); release();
  await expect(enabled).toHaveAttribute("aria-pressed", "true");
  await expect(search).toBeFocused();
  expect(commands.filter(command => command === "subscribe_channel").length).toBe(subscriptions);
  await group.getByRole("button", { name: "# Muted", exact: true }).click();
  await expect(composer).toHaveValue("Keep my draft");
});

test("React preview changes channel notifications with host pending and local retry", async ({ page }) => {
  let releaseFailure = () => {};
  const failureGate = new Promise<void>(resolve => { releaseFailure = resolve; });
  const methods: string[] = [];
  let attempts = 0;
  await page.route("**/api/v1/channels/*/notifications**", async route => {
    methods.push(route.request().method());
    attempts++;
    if (attempts === 1) {
      await failureGate;
      await route.fulfill({ status: 503, contentType: "text/plain", body: "Mellombels utilgjengeleg" });
      return;
    }
    await route.continue();
  });

  await page.goto("/?participant=playwright-preview-channel-notifications&ui=react", { waitUntil: "domcontentloaded" });
  const preview = page.locator("#sproyt-react-preview");
  await expect(page.locator("#status")).toHaveText(/Tilkopla/, { timeout: 15_000 });
  if (await preview.locator(".sp-sproyt-brand-action").isVisible()) {
    await preview.getByRole("button", { name: "Samtalar", exact: true }).click();
  }
  const toggle = preview.getByRole("button", { name: /Slå (?:på|av) varsel for general/i });
  await expect(toggle).toBeVisible();
  const initial = await toggle.getAttribute("aria-pressed");
  const expectedMethod = initial === "true" ? "DELETE" : "PUT";

  await toggle.click();
  await expect(toggle).toBeDisabled();
  await expect(toggle).toHaveAttribute("aria-busy", "true");
  releaseFailure();
  const error = preview.getByRole("alert").filter({ hasText: "Kunne ikkje endre kanalvarsel" });
  await expect(error).toContainText("Mellombels utilgjengeleg");
  await expect(toggle).toHaveAttribute("aria-pressed", initial ?? "false");
  await expect(toggle).toBeEnabled();

  await error.getByRole("button", { name: "Prøv igjen" }).click();
  await expect(error).toHaveCount(0);
  await expect(toggle).toHaveAttribute("aria-pressed", initial === "true" ? "false" : "true");
  expect(methods).toEqual([expectedMethod, expectedMethod]);
});
