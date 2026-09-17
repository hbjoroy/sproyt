import { expect, test, type Page } from "@playwright/test";

const mediaId = "c63ac052-a05a-4b5d-bfff-04429338df90";
type Job = { id: string; channel_id: string; prompt: string; state: string; [key: string]: unknown };

async function fixture(page: Page) {
  let job: Job | null = null;
  let failReview = false;
  let holdReview: Promise<void> = Promise.resolve();
  const sent: { type: string; payload: { body?: string } }[] = [];
  const submissions: { channel_id: string; prompt: string; request_id: string; reference_ids: string[] }[] = [];
  let sockets = 0;
  page.on("websocket", socket => {
    sockets++;
    socket.on("framesent", frame => sent.push(JSON.parse(String(frame.payload))));
  });
  await page.route(/\/api\/v1\/imagegen(?:\?.*)?$/, async route => {
    if (route.request().method() === "POST") {
      const request = route.request().postDataJSON(); submissions.push(request);
      job = { id: "react-job", channel_id: request.channel_id, prompt: request.prompt, state: "ready",
        expansion: { prompt: "A careful composition", model: "test-model", style: "painting", warning: "Check details", sources: ["https://en.wikipedia.org/wiki/Paros", "javascript:alert(1)"] },
        visual_references: [{ title: "Paros photo", url: "https://commons.wikimedia.org/wiki/File:Paros.jpg", credit: "Photographer · CC BY" }, { title: "unsafe", url: "javascript:alert(1)", credit: "none" }] };
      return route.fulfill({ json: { job } });
    }
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
  return { sent, submissions, sockets: () => sockets, fail: () => { failReview = true; }, hold: (promise: Promise<void>) => { holdReview = promise; } };
}

test("private image review survives reload and attaches without publishing; details, safe sources, hide, retry and dismiss", async ({ page }) => {
  const state = await fixture(page);
  await page.goto("/?participant=react-image-review&ui=react");
  const preview = page.locator("#sproyt-react-preview");
  const input = preview.getByRole("textbox", { name: "Skriv melding" });
  const inbox = preview.getByRole("region", { name: "Private biletmeldingar" });
  await expect(input).toBeEnabled({ timeout: 15000 });
  await input.fill('/imagegen "A sea view"');
  await input.press("Enter");
  await expect(inbox.getByRole("button", { name: "Godta", exact: true })).toBeEnabled();
  await expect(input).toHaveValue("");
  await inbox.getByText("Sjå utvida biletprompt").click();
  await expect(inbox).toContainText("A careful composition");
  await expect(inbox.getByRole("link", { name: "Kjelde: Paros" })).toHaveAttribute("href", "https://en.wikipedia.org/wiki/Paros");
  await inbox.getByText("Sjå referansefoto").click();
  await expect(inbox.getByRole("link", { name: "Paros photo" })).toBeVisible();
  await expect(inbox.locator('a[href^="javascript:"]')).toHaveCount(0);
  await inbox.getByRole("button", { name: "Skjul", exact: true }).click();
  await expect(inbox).toHaveCount(0);
  await input.focus();
  await preview.getByRole("button", { name: "Biletegenerering", exact: true }).click();
  await expect(inbox).toBeVisible();
  expect(state.sockets()).toBe(1);
  await page.reload();
  await expect(inbox.getByRole("button", { name: "Godta", exact: true })).toBeEnabled({ timeout: 15000 });
  await input.fill("Caption remains private");
  state.fail();
  await inbox.getByRole("button", { name: "Godta", exact: true }).click();
  await expect(inbox).toContainText("Prøv gjennomgangen igjen");
  await inbox.getByRole("button", { name: "Godta", exact: true }).click();
  await expect(preview.getByRole("button", { name: "Fjern generated.png" })).toBeEnabled();
  await expect(input).toHaveValue("Caption remains private");
  await expect(inbox).toContainText("Biletet er lagt i utkastet");
  expect(state.sent.filter(command => command.type === "send_message")).toEqual([]);
  await inbox.getByRole("button", { name: "Lukk", exact: true }).click();
  await expect(inbox.locator("article")).toHaveCount(0);
  await expect(preview.getByRole("button", { name: "Fjern generated.png" })).toBeVisible();
});

test("decline publishes nothing and a delayed accept cannot attach to a different channel", async ({ page }) => {
  await page.addInitScript(() => {
    const NativeSocket = WebSocket;
    window.WebSocket = class extends NativeSocket {
      constructor(url: string | URL, protocols?: string | string[]) {
        super(url, protocols); (window as unknown as { testSocket: WebSocket }).testSocket = this;
      }
    };
  });
  const state = await fixture(page);
  await page.goto("/?participant=react-image-origin&ui=react");
  const preview = page.locator("#sproyt-react-preview");
  const input = preview.getByRole("textbox", { name: "Skriv melding" });
  const inbox = preview.getByRole("region", { name: "Private biletmeldingar" });
  await expect(input).toBeEnabled({ timeout: 15000 });
  const name = `Image-${Date.now()}`;
  await page.evaluate(name => (window as unknown as { testSocket: WebSocket }).testSocket.send(JSON.stringify({ protocol: "sproyt.chat.v1", type: "create_channel", request_id: crypto.randomUUID(), payload: { name, slug: name.toLowerCase(), kind: "public" } })), name);
  await expect(preview.getByRole("heading", { name: `# ${name}`, exact: true })).toBeVisible();
  await input.fill('/imagegen "Decline me"'); await input.press("Enter");
  await inbox.getByRole("button", { name: "Avslå", exact: true }).click();
  await expect(inbox.locator("article")).toHaveCount(0);
  await expect(preview.getByRole("button", { name: "Fjern generated.png" })).toHaveCount(0);
  await input.fill('/imagegen "Accept later"'); await input.press("Enter");
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

test("thread image command uses its references and review restores only that thread draft", async ({ page }) => {
  const state = await fixture(page);
  await page.route(/\/channels\/[^/]+\/media(?:\?.*)?$/, route => route.fulfill({ json: { media: {
    id: "337b2426-5377-4f25-93b5-832cd8d6e201", channel_id: new URL(route.request().url()).pathname.split("/")[4],
    original_filename: "reference.png", content_type: "image/png"
  } } }));
  await page.goto("/?participant=react-image-thread&ui=react");
  const preview = page.locator("#sproyt-react-preview");
  const input = preview.getByRole("textbox", { name: "Skriv melding" });
  await expect(input).toBeEnabled({ timeout: 15000 });
  await input.fill("Image thread root"); await input.press("Enter");
  await preview.locator("[data-message-id]").filter({ hasText: "Image thread root" }).getByRole("button", { name: "Svar i tråd", exact: true }).click();
  const thread = preview.locator(".sp-thread-pane");
  const reply = thread.getByRole("textbox", { name: "Svar i tråden" });
  await thread.locator('input[type="file"]').setInputFiles({ name: "reference.png", mimeType: "image/png", buffer: Buffer.from([0x89, 0x50, 0x4e, 0x47]) });
  await expect(thread.getByRole("button", { name: "Fjern reference.png" })).toBeEnabled();
  await reply.fill('/imagegen "Use the reference"'); await reply.press("Enter");
  await expect(reply).toHaveValue("");
  expect(state.submissions[0]?.reference_ids).toEqual(["337b2426-5377-4f25-93b5-832cd8d6e201"]);
  await reply.fill("Private thread caption");
  await page.reload();
  await expect(input).toBeEnabled({ timeout: 15000 });
  await preview.locator("[data-message-id]").filter({ hasText: "Image thread root" }).getByRole("button", { name: "Svar i tråd", exact: true }).click();
  await expect(reply).toHaveValue("Private thread caption");
  await expect(thread.getByRole("region", { name: "Private biletmeldingar" })).toContainText("Trådutkast", { timeout: 15000 });
  await thread.getByRole("button", { name: "Godta", exact: true }).click();
  await expect(thread.getByRole("button", { name: "Fjern generated.png" })).toBeEnabled();
  await expect(reply).toHaveValue("Private thread caption");
  await expect(preview.locator(".sp-channel-pane").getByRole("button", { name: "Fjern generated.png" })).toHaveCount(0);
  expect(state.sent.filter(command => command.type === "send_message").map(command => command.payload.body)).toEqual(["Image thread root"]);
});
