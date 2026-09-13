import { expect, test } from "@playwright/test";

test("imagegen stays private, survives reload, and accepts into a draft without sending", async ({ page }) => {
  let job: Record<string, unknown> | null = null;
  const sent: string[] = [];
  page.on("websocket", socket => socket.on("framesent", frame => { sent.push(String(frame.payload)); }));
  await page.route(/\/api\/v1\/imagegen(?:\?.*)?$/, async route => {
    if (route.request().method() === "POST") {
      const body = route.request().postDataJSON() as { prompt: string; channel_id: string };
      job = { id: "test-image", channel_id: body.channel_id, state: "ready", prompt: body.prompt, error: null,
        visual_references: [
          { title: "Artemis ved Paros", url: "https://commons.wikimedia.org/wiki/File:20221101_440_Paros.jpg", credit: "Jean Housen · CC BY-SA 4.0" },
          { title: "Untrusted", url: "javascript:alert(1)", credit: "Unknown" }
        ] };
      await route.fulfill({ json: { job } });
    } else await route.fulfill({ json: { enabled: true, jobs: job ? [job] : [] } });
  });
  await page.route(/\/api\/v1\/imagegen\/test-image\/preview/, route => route.fulfill({ contentType: "image/png", body: Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=", "base64") }));
  await page.route(/\/api\/v1\/imagegen\/test-image\/review/, async route => {
    const { decision } = route.request().postDataJSON() as { decision: string };
    if (!job) throw new Error("missing job");
    if (decision === "decline") { job = null; await route.fulfill({ json: {} }); return; }
    job.state = "accepted";
    await route.fulfill({ json: { job, media: { id: "c63ac052-a05a-4b5d-bfff-04429338df90", channel_id: job.channel_id, original_filename: "generated.png", content_type: "image/png" } } });
  });
  await page.goto("/?participant=imagegen-browser-test");
  await expect(page.locator("#status")).toHaveText(/Tilkopla/, { timeout: 15000 });
  await page.locator("#body").fill('/imagegen "An oil painting of the sea"');
  await page.locator("#send-form").evaluate((form: HTMLFormElement) => form.requestSubmit());
  const inbox = page.getByRole("region", { name: "Private biletmeldingar" });
  await expect(inbox.getByRole("button", { name: "Godta", exact: true })).toBeVisible();
  await inbox.getByText("Sjå referansefoto", { exact: true }).click();
  await expect(inbox.getByRole("link", { name: "Artemis ved Paros" })).toHaveAttribute("href", "https://commons.wikimedia.org/wiki/File:20221101_440_Paros.jpg");
  await expect(inbox.getByRole("link", { name: "Untrusted" })).toHaveCount(0);
  expect(sent.some(frame => frame.includes("send_message") && frame.includes("oil painting"))).toBe(false);
  await page.reload();
  await expect(inbox.getByRole("button", { name: "Godta", exact: true })).toBeVisible({ timeout: 15000 });
  await page.locator("#body").fill("My caption");
  await inbox.getByRole("button", { name: "Godta", exact: true }).click();
  await expect(page.locator("#body")).toHaveValue("My caption");
  await expect(page.locator(".media-preview-label").filter({ hasText: "generated.png" })).toBeVisible();
  expect(sent.some(frame => frame.includes("send_message") && frame.includes("c63ac052"))).toBe(false);
});

test("declining a generated image adds no attachment", async ({ page }) => {
  let declined = false;
  let channel = "";
  await page.route(/\/api\/v1\/imagegen(?:\?.*)?$/, async route => {
    if (route.request().method() === "POST") channel = (route.request().postDataJSON() as {channel_id:string}).channel_id;
    await route.fulfill({ json: { enabled: true, jobs: channel && !declined ? [{ id: "decline-image", channel_id: channel, state: "ready", prompt: "Seascape", error: null }] : [] } });
  });
  await page.route(/\/api\/v1\/imagegen\/decline-image\/preview/, route => route.fulfill({ status: 204 }));
  await page.route(/\/api\/v1\/imagegen\/decline-image\/review/, async route => { declined = true; await route.fulfill({ json: {} }); });
  await page.goto("/?participant=imagegen-decline-test");
  await expect(page.locator("#status")).toHaveText(/Tilkopla/, { timeout: 15000 });
  await page.locator("#body").fill('/imagegen "Seascape"');
  await page.locator("#send-form").evaluate((form: HTMLFormElement) => form.requestSubmit());
  await page.getByRole("button", { name: "Avslå", exact: true }).click();
  await expect(page.getByRole("button", { name: "Godta", exact: true })).toHaveCount(0);
  await expect(page.locator(".media-preview-label")).toHaveCount(0);
  expect(declined).toBe(true);
});
