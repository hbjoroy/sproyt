import { expect, test } from "@playwright/test";

const ids = ["c63ac052-a05a-4b5d-bfff-04429338df90", "c63ac052-a05a-4b5d-bfff-04429338df91", "c63ac052-a05a-4b5d-bfff-04429338df92"];

test("compact information tasks preserve drafts and distinguish requester from reviewer controls", async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.addInitScript(() => {
    const NativeSocket = WebSocket;
    window.WebSocket = class extends NativeSocket {
      constructor(url: string | URL, protocols?: string | string[]) {
        super(url, protocols); (window as unknown as { testSocket: WebSocket }).testSocket = this;
      }
    };
  });
  const submissions: Record<string, unknown>[] = [];
  const completed = new Set<string>();
  let fail = true;
  await page.route(/\/api\/v1\/work-item-tasks\//, route => {
    const request = route.request();
    const id = /\/work-item-tasks\/([^/?]+)/.exec(new URL(request.url()).pathname)?.[1] ?? ids[0]!;
    const index = ids.indexOf(id);
    let messageId = new URL(request.url()).searchParams.get("message_id");
    if (request.method() === "POST") {
      const body = request.postDataJSON() as Record<string, unknown>; submissions.push(body); messageId = String(body.message_id);
      if (fail) { fail = false; return route.fulfill({ status: 503, body: "Tenesta er mellombels utilgjengeleg." }); }
      completed.add(id);
    }
    return route.fulfill({ json: { id, message_id: messageId, work_item_id: ids[0], revision: completed.has(id) ? 2 : 1,
      application_name: "Sprøyt", title: "Skrivefelt på mobil", description: "Feltet forsvinn i Edge", status: completed.has(id) ? "completed" : "pending",
      process_status: "waiting", delivery_status: "ready", category: index === 2 ? "bug" : null, priority: index === 2 ? "high" : null, decision_status: null,
      node_id: ["review", "provide-information", "followup-review"][index], can_request_information: index === 0,
      information_request: "Kva nettlesar og versjon?", information_response: index === 2 ? "Edge på Android 16" : null,
      assignee_name: index === 1 ? "Innmeldar" : "Harald", can_decide: index !== 0 && !completed.has(id), blocked: false } });
  });
  await page.goto("/?participant=work-item-information-test&ui=react");
  const preview = page.locator("#sproyt-react-preview");
  const input = preview.getByRole("textbox", { name: "Skriv melding" });
  await expect(input).toBeEnabled({ timeout: 15000 });
  const name = `Information-${Date.now()}`;
  await page.evaluate(name => (window as unknown as { testSocket: WebSocket }).testSocket.send(JSON.stringify({
    protocol: "sproyt.chat.v1", type: "create_channel", request_id: crypto.randomUUID(), payload: { name, slug: name.toLowerCase(), kind: "public" }
  })), name);
  await expect(preview.getByRole("region", { name: `# ${name}`, exact: true })).toBeVisible();
  await expect(input).toBeEnabled();
  for (const id of ids) {
    await input.fill(`[[work-item-task:${id}]]`);
    await preview.getByRole("button", { name: /^Send / }).click();
    await expect(input).toHaveValue("");
  }
  const cards = preview.locator(".sp-work-item-task");
  await expect(cards).toHaveCount(3);
  const review = cards.nth(0); const information = cards.nth(1); const followup = cards.nth(2);
  await review.locator(".sp-work-item-task-summary").click();
  await expect(review.locator("form")).toHaveCount(0);
  await information.locator(".sp-work-item-task-summary").click();
  await expect(information).toContainText("Kva nettlesar og versjon?");
  await expect(information.getByRole("combobox")).toHaveCount(0);
  const send = information.getByRole("button", { name: "Send svar", exact: true });
  await expect(send).toBeDisabled();
  await information.getByRole("textbox", { name: "Svar til behandlar" }).fill("Edge på Android 16");
  await input.fill("Utkastet skal bli verande");
  await send.click();
  await expect(information).toContainText("mellombels utilgjengeleg");
  await expect(information.getByRole("textbox", { name: "Svar til behandlar" })).toHaveValue("Edge på Android 16");
  await send.click();
  await expect(information.locator(".sp-work-item-task-summary")).toContainText("Fullført");
  expect(submissions).toHaveLength(2); expect(submissions[0]).toEqual(submissions[1]);
  await expect(input).toHaveValue("Utkastet skal bli verande");
  await followup.locator(".sp-work-item-task-summary").click();
  await expect(followup).toContainText("Edge på Android 16");
  await expect(followup.getByRole("combobox", { name: "Prioritet" })).toHaveValue("high");
  await expect(followup.getByRole("combobox", { name: "Avgjerd" }).locator('option[value="needs_information"]')).toHaveCount(0);
  await expect(followup.getByRole("button", { name: "Lagre avgjerd" })).toBeEnabled();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  await followup.screenshot({ path: testInfo.outputPath("work-item-information-compact.png") });
});
