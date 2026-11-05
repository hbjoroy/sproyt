import { expect, test, type Page } from "@playwright/test";
import { readFile } from "node:fs/promises";
import { createChannelViaUi } from "./support/channel-setup";

// The existing review flows need the wide conversation navigation. Compact layout is exercised separately below.
test.use({ viewport: { width: 1280, height: 800 } });

const mediaId = "c63ac052-a05a-4b5d-bfff-04429338df90";
type Job = { id: string; channel_id: string; prompt: string; state: string; [key: string]: unknown };

async function fixture(page: Page) {
  const bundle = await readFile(new URL("../dist/app.js", import.meta.url), "utf8");
  await page.route(/\/assets\/app\/[^/]+\/app.js$/, route => route.fulfill({ contentType: "text/javascript", body: bundle }));
  let polls = 0;
  let holdPoll: Promise<void> = Promise.resolve();
  let job: Job | null = null;
  let failReview = false;
  let holdReview: Promise<void> = Promise.resolve();
  const sent: { type: string; payload: { body?: string } }[] = [];
  const submissions: { channel_id: string; prompt: string; mode: string; request_id: string; reference_ids: string[] }[] = [];
  let sockets = 0;
  page.on("websocket", socket => {
    sockets++;
    socket.on("framesent", frame => sent.push(JSON.parse(String(frame.payload))));
  });
  await page.route(/\/api\/v1\/imagegen(?:\?.*)?$/, async route => {
    if (route.request().method() === "POST") {
      const request = route.request().postDataJSON(); submissions.push(request);
      job = { id: "react-job", channel_id: request.channel_id, prompt: request.prompt, mode: request.mode, state: "ready",
        expansion: { prompt: "A careful composition", model: "test-model", style: "painting", warning: "Check details", sources: ["https://en.wikipedia.org/wiki/Paros", "javascript:alert(1)"] },
        visual_references: [{ title: "Paros photo", url: "https://commons.wikimedia.org/wiki/File:Paros.jpg", credit: "Photographer · CC BY" }, { title: "unsafe", url: "javascript:alert(1)", credit: "none" }] };
      return route.fulfill({ json: { job } });
    }
    polls++;
    await holdPoll;
    return route.fulfill({ json: { jobs: job ? [job] : [] } });
  });
  await page.route(/\/api\/v1\/imagegen\/react-job\/preview/, route => route.fulfill({ status: 204 }));
  await page.route(/\/api\/v1\/imagegen\/react-job\/review/, async route => {
    await holdReview;
    if (failReview) { failReview = false; return route.fulfill({ status: 503, json: { error: "Prøv gjennomgangen igjen" } }); }
    const decision = route.request().postDataJSON().decision;
    if (!job) throw new Error("Missing fixture job");
    if (decision !== "accept") { job = null; return route.fulfill({ json: {} }); }
    job.state = "accepted";
    return route.fulfill({ json: { job, media: { id: mediaId, channel_id: job.channel_id, original_filename: "generated.png", content_type: "image/png" } } });
  });
  return { sent, submissions, polls: () => polls, holdPoll: (promise: Promise<void>) => { holdPoll = promise; }, sockets: () => sockets, fail: () => { failReview = true; }, hold: (promise: Promise<void>) => { holdReview = promise; } };
}

test("private image review survives reload and attaches without publishing; details, safe sources, hide, retry and dismiss", async ({ page }) => {
  const state = await fixture(page);
  await page.goto("/?participant=react-image-review&ui=react");
  const preview = page.locator("#sproyt-react-preview");
  const input = preview.getByRole("textbox", { name: "Skriv melding" });
  const inbox = preview.getByRole("region", { name: "Private biletmeldingar" });
  await expect(input).toBeEnabled({ timeout: 15000 });
  await input.fill('/imagegen "A sea view"');
  await input.press("Control+Enter");
  await expect(inbox.getByRole("button", { name: "Godta", exact: true })).toBeEnabled();
  await expect(input).toHaveValue("");
  await inbox.getByText("Sjå innsend biletprompt (expanded)").click();
  await expect(inbox).toContainText("A careful composition");
  await expect(inbox.getByRole("link", { name: "Kjelde: Paros" })).toHaveAttribute("href", "https://en.wikipedia.org/wiki/Paros");
  await inbox.getByText("Sjå referansefoto").click();
  await expect(inbox.getByRole("link", { name: "Paros photo" })).toBeVisible();
  await expect(inbox.locator('a[href^="javascript:"]')).toHaveCount(0);
  await inbox.getByRole("button", { name: "Skjul", exact: true }).click();
  await expect(inbox).toHaveCount(0);
  await input.focus();
  await preview.getByRole("button", { name: "Skriveverktøy", exact: true }).click();
  await preview.getByRole("button", { name: "Biletegenerering", exact: true }).click();
  await expect(inbox).toBeVisible();
  expect(state.sockets()).toBe(1);
  await page.reload();
  await expect(inbox.getByRole("button", { name: "Godta", exact: true })).toBeEnabled({ timeout: 15000 });
  await input.fill("Caption remains private");
  state.fail();
  await inbox.getByRole("button", { name: "Godta", exact: true }).click();
  await expect(inbox).toContainText("Prøv gjennomgangen igjen");
  let release!: () => void;
  state.hold(new Promise<void>(resolve => { release = resolve; }));
  let releasePoll!: () => void;
  state.holdPoll(new Promise<void>(resolve => { releasePoll = resolve; }));
  const accept = inbox.getByRole("button", { name: "Godta", exact: true });
  await accept.focus(); await accept.press("Enter");
  await expect(inbox).toBeVisible();
  await expect(inbox.getByRole("button", { name: "Godta", exact: true })).toBeDisabled();
  release();
  await expect(preview.getByRole("button", { name: "Fjern generated.png" })).toBeEnabled();
  await expect(input).toHaveValue("Caption remains private");
  await expect(inbox).toHaveCount(0);
  await expect(input).toBeFocused();
  releasePoll();
  const polls = state.polls();
  await expect.poll(state.polls, { timeout: 8000 }).toBeGreaterThan(polls);
  await expect(inbox).toHaveCount(0);
  expect(state.sent.filter(command => command.type === "send_message")).toEqual([]);
  await input.focus();
  await preview.getByRole("button", { name: "Skriveverktøy", exact: true }).click();
  await preview.getByRole("button", { name: "Biletegenerering", exact: true }).click();
  await expect(inbox.getByRole("button", { name: "Legg i utkast", exact: true })).toBeEnabled();
  await inbox.getByRole("button", { name: "Lukk", exact: true }).click();
  await expect(inbox.locator("article")).toHaveCount(0);
  await expect(preview.getByRole("button", { name: "Fjern generated.png" })).toBeVisible();
});

test("decline publishes nothing and a delayed accept cannot attach to a different channel", async ({ page }) => {
  const state = await fixture(page);
  await page.goto("/?participant=react-image-origin&ui=react");
  const preview = page.locator("#sproyt-react-preview");
  const input = preview.getByRole("textbox", { name: "Skriv melding" });
  const inbox = preview.getByRole("region", { name: "Private biletmeldingar" });
  await expect(input).toBeEnabled({ timeout: 15000 });
  const name = `Image-${Date.now()}`;
  await createChannelViaUi(page, name);
  await expect(preview.getByRole("heading", { name: `# ${name}`, exact: true })).toBeVisible();
  await input.fill('/imagegen "Decline me"'); await input.press("Control+Enter");
  await inbox.getByRole("button", { name: "Avslå", exact: true }).click();
  await expect(inbox.locator("article")).toHaveCount(0);
  await expect(preview.getByRole("button", { name: "Fjern generated.png" })).toHaveCount(0);
  await input.fill('/imagegen "Accept later"'); await input.press("Control+Enter");
  let release!: () => void;
  state.hold(new Promise<void>(resolve => { release = resolve; }));
  await inbox.getByRole("button", { name: "Godta", exact: true }).click();
  await preview.getByRole("button", { name: /# general/i }).click();
  release();
  await expect(inbox).toContainText("Opne den opphavlege kanalen");
  await expect(preview.getByRole("button", { name: "Fjern generated.png" })).toHaveCount(0);
  await expect(inbox.getByRole("button", { name: "Legg i utkast", exact: true })).toBeDisabled();
  expect(state.sent.filter(command => command.type === "send_message")).toEqual([]);
});

test("thread image command uses its references and review restores only that thread draft", async ({ page }, testInfo) => {
  const state = await fixture(page);
  const rootTitle = `Image thread root ${testInfo.project.name}`;
  await page.route(/\/channels\/[^/]+\/media(?:\?.*)?$/, route => route.fulfill({ json: { media: {
    id: "337b2426-5377-4f25-93b5-832cd8d6e201", channel_id: new URL(route.request().url()).pathname.split("/")[4],
    original_filename: "reference.png", content_type: "image/png"
  } } }));
  await page.goto("/?participant=react-image-thread&ui=react");
  const preview = page.locator("#sproyt-react-preview");
  const input = preview.getByRole("textbox", { name: "Skriv melding" });
  await expect(input).toBeEnabled({ timeout: 15000 });
  await input.fill(rootTitle); await input.press("Control+Enter");
  await preview.locator("[data-message-id]").filter({ hasText: rootTitle }).getByRole("button", { name: "Svar i tråd", exact: true }).click();
  const thread = preview.locator(".sp-thread-pane");
  const reply = thread.getByRole("textbox", { name: "Svar i tråden" });
  await thread.locator('input[type="file"]').setInputFiles({ name: "reference.png", mimeType: "image/png", buffer: Buffer.from([0x89, 0x50, 0x4e, 0x47]) });
  await expect(thread.getByRole("button", { name: "Fjern reference.png" })).toBeEnabled();
  await reply.fill('/imagegen "Use the reference"'); await reply.press("Control+Enter");
  await expect(reply).toHaveValue("");
  expect(state.submissions[0]?.reference_ids).toEqual(["337b2426-5377-4f25-93b5-832cd8d6e201"]);
  await reply.fill("Private thread caption");
  await page.reload();
  await expect(input).toBeEnabled({ timeout: 15000 });
  await preview.locator("[data-message-id]").filter({ hasText: rootTitle }).getByRole("button", { name: "Svar i tråd", exact: true }).click();
  await expect(reply).toHaveValue("Private thread caption");
  await expect(thread.getByRole("region", { name: "Private biletmeldingar" })).toContainText("Trådutkast", { timeout: 15000 });
  await thread.getByRole("button", { name: "Godta", exact: true }).click();
  await expect(thread.getByRole("button", { name: "Fjern generated.png" })).toBeEnabled();
  await expect(thread.getByRole("region", { name: "Private biletmeldingar" })).toHaveCount(0);
  await expect(reply).toHaveValue("Private thread caption");
  await expect(preview.locator(".sp-channel-pane").getByRole("button", { name: "Fjern generated.png" })).toHaveCount(0);
  expect(state.sent.filter(command => command.type === "send_message").map(command => command.payload.body)).toEqual([rootTitle]);
});

test("literal command carries mode and displays submitted prompt details", async ({ page }) => {
  const state = await fixture(page);
  await page.goto("/?participant=react-image-literal&ui=react");
  const preview = page.locator("#sproyt-react-preview");
  const input = preview.getByRole("textbox", { name: "Skriv melding" });
  await expect(input).toBeEnabled({ timeout: 15000 });
  await input.fill('/imagegen literal "Keep background. Add Maria."');
  await input.press("Control+Enter");
  const inbox = preview.getByRole("region", { name: "Private biletmeldingar" });
  await expect(inbox.getByText("Sjå innsend biletprompt (literal)")).toBeVisible();
  expect(state.submissions[0]?.mode).toBe("literal");
  expect(state.submissions[0]?.prompt).toBe("Keep background. Add Maria.");
});


for (const width of [320, 390, 1440]) for (const colorScheme of ["light", "dark"] as const) {
  test(`workshop stays distinct with a reachable hide button at ${width}px in ${colorScheme}`, async ({ page }, testInfo) => {
    await page.setViewportSize({ width, height: 900 });
    await page.emulateMedia({ colorScheme });
    await fixture(page);
    await page.route(/\/api\/v1\/imagegen\/react-job\/preview/, route => route.fulfill({ contentType: "image/svg+xml",
      body: '<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="800"><rect width="1200" height="800" fill="#598d87"/><circle cx="800" cy="250" r="110" fill="#f3d58a"/></svg>' }));
    await page.goto(`/?participant=workshop-design-${width}-${colorScheme}&ui=react`);
    const preview = page.locator("#sproyt-react-preview");
    const input = preview.getByRole("textbox", { name: "Skriv melding" });
    await expect(input).toBeEnabled({ timeout: 15000 });
    await input.fill(`/imagegen "${"Eit roleg landskap med hav og fjell. ".repeat(18)}"`);
    await preview.getByRole("button", { name: "Send ↑", exact: true }).click();
    const inbox = preview.getByRole("region", { name: "Private biletmeldingar" });
    const hide = inbox.getByRole("button", { name: "Skjul", exact: true });
    await expect(hide).toBeVisible();
    await expect(hide).toHaveCSS("border-top-width", "1px");
    await expect(hide).toHaveCSS("text-decoration-line", "none");
    expect((await hide.boundingBox())!.height).toBeGreaterThanOrEqual(44);
    expect(await inbox.evaluate(element => getComputedStyle(element).backgroundColor)).not.toBe(await preview.locator(".sp-channel-pane").evaluate(element => getComputedStyle(element).backgroundColor));
    await input.fill("Meldingsutkastet er bevart");
    const body = inbox.locator(".sp-imagegen-body");
    await expect.poll(() => body.evaluate(element => element.scrollHeight > element.clientHeight)).toBe(true);
    await body.evaluate(element => { element.scrollTop = element.scrollHeight; });
    const headerBounds = (await inbox.locator("header").boundingBox())!;
    const panelBounds = (await inbox.boundingBox())!;
    expect(headerBounds.y).toBeGreaterThanOrEqual(panelBounds.y);
    expect(headerBounds.y + headerBounds.height).toBeLessThanOrEqual(panelBounds.y + panelBounds.height);
    await expect(hide).toBeInViewport();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
    await page.screenshot({ path: testInfo.outputPath(`workshop-${width}-${colorScheme}.png`) });
    await hide.focus(); await hide.press("Enter");
    await expect(inbox).toHaveCount(0);
    await expect(input).toBeFocused();
    await expect(input).toHaveValue("Meldingsutkastet er bevart");
  });
}

test("a generated image is reviewed and attached in the personal chat log", async ({ page }) => {
  const state = await fixture(page);
  await page.goto("/?participant=react-image-personal");
  const app = page.locator("#sproyt-react-preview");
  await app.getByRole("button", { name: "Området mitt", exact: true }).click();
  await expect(app.getByRole("heading", { name: "Området mitt", exact: true })).toBeVisible();
  const personalChannel = new URL(page.url()).searchParams.get("channel");
  const input = app.getByRole("textbox", { name: "Skriv melding", exact: true });
  await expect(input).toBeEnabled();
  await input.fill('/imagegen literal "A private seascape"');
  await input.press("Enter");
  const inbox = app.getByRole("region", { name: "Private biletmeldingar" });
  await inbox.getByRole("button", { name: "Godta", exact: true }).click();
  await expect(app.getByRole("button", { name: "Fjern generated.png" })).toBeEnabled();
  expect(state.submissions[0]?.channel_id).toBe(personalChannel);
  expect(state.sent.filter(command => command.type === "send_message")).toEqual([]);
});
