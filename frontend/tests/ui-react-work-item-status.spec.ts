import { expect, test } from "@playwright/test";

const reviewId = "c63ac052-a05a-4b5d-bfff-04429338df90";
const statusTaskId = "c63ac052-a05a-4b5d-bfff-04429338df91";
const itemId = "c63ac052-a05a-4b5d-bfff-04429338df92";
const hiddenItemId = "c63ac052-a05a-4b5d-bfff-04429338df93";

test("mobile status tasks preserve a retry and public cards hide private data", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.addInitScript(() => {
    const NativeSocket = WebSocket;
    window.WebSocket = class extends NativeSocket {
      constructor(url: string | URL, protocols?: string | string[]) {
        super(url, protocols); (window as unknown as { testSocket: WebSocket }).testSocket = this;
      }
    };
  });
  const statusBodies: Record<string, unknown>[] = [];
  const startBodies: Record<string, unknown>[] = [];
  let fail = true;
  await page.route(/\/api\/v1\/(work-item-tasks|work-items)\//, route => {
    const request = route.request();
    const url = new URL(request.url());
    if (url.pathname.endsWith("/status-change")) {
      startBodies.push(request.postDataJSON() as Record<string, unknown>);
      return route.fulfill({ json: { id: statusTaskId, work_item_id: itemId, channel_name: "Review", start_status: "pending" } });
    }
    if (url.pathname.endsWith("/status") && url.pathname.includes("/work-item-tasks/")) {
      statusBodies.push(request.postDataJSON() as Record<string, unknown>);
      if (fail) { fail = false; return route.fulfill({ status: 503, body: "Tenesta er mellombels utilgjengeleg." }); }
    }
    if (url.pathname.endsWith("/status") && url.pathname.includes("/work-items/")) {
      expect(url.searchParams.get("message_id")).toMatch(/^[0-9a-f-]{36}$/);
      if (url.pathname.includes(hiddenItemId)) return route.fulfill({ json: { visible: false } });
      return route.fulfill({ json: { visible: true, title: "Skrivefelt på mobil", application_name: "Sprøyt",
        status: "in_development", public_feedback: "Vi arbeider med saka", history: [
          { from_status: "planned", to_status: "in_development", created_at: 123, public_feedback: "Vi arbeider med saka" }
        ] } });
    }
    const taskId = url.pathname.includes(statusTaskId) ? statusTaskId : reviewId;
    const body = request.method() === "POST" ? request.postDataJSON() as Record<string, unknown> : null;
    const messageId = body ? String(body.message_id) : url.searchParams.get("message_id");
    return route.fulfill({ json: { id: taskId, message_id: messageId, work_item_id: itemId,
      revision: statusBodies.length > 1 ? 2 : 1, application_name: "Sprøyt", title: "Skrivefelt på mobil",
      description: "Feltet forsvinn i Edge", status: taskId === reviewId || statusBodies.length > 1 ? "completed" : "pending",
      process_status: "completed", delivery_status: statusBodies.length > 1 ? "pending" : "ready",
      category: "bug", priority: "normal", decision_status: "planned", assignee_name: "Harald",
      can_decide: taskId === statusTaskId && statusBodies.length < 2, blocked: false,
      node_id: taskId === statusTaskId ? "change-status" : "review", can_request_information: false,
      information_request: null, information_response: null,
      lifecycle: { case_status: "planned", can_start: taskId === reviewId, allowed_statuses: ["in_development", "resolved", "rejected"],
        internal_note: null, public_feedback: null, history: [] } } });
  });
  await page.goto("/?participant=work-item-status-test&ui=react");
  const preview = page.locator("#sproyt-react-preview");
  const input = preview.getByRole("textbox", { name: "Skriv melding" });
  await expect(input).toBeEnabled({ timeout: 15000 });
  const name = `Status-${Date.now()}`;
  await page.evaluate(name => (window as unknown as { testSocket: WebSocket }).testSocket.send(JSON.stringify({
    protocol: "sproyt.chat.v1", type: "create_channel", request_id: crypto.randomUUID(),
    payload: { name, slug: name.toLowerCase(), kind: "public" }
  })), name);
  await expect(preview.getByRole("region", { name: `# ${name}`, exact: true })).toBeVisible();
  for (const marker of [
    `[[work-item-task:${reviewId}]]`, `[[work-item-task:${statusTaskId}]]`,
    `[[work-item-status:${itemId}]]`, `[[work-item-status:${hiddenItemId}]]`
  ]) {
    await input.fill(marker); await preview.getByRole("button", { name: /^Send / }).click();
    await expect(input).toHaveValue("");
  }
  const cards = preview.locator(".sp-work-item-task");
  await expect(cards).toHaveCount(4);
  const review = cards.nth(0); const change = cards.nth(1); const publicCard = cards.nth(2); const hidden = cards.nth(3);
  await review.locator(".sp-work-item-task-summary").click();
  await review.getByRole("button", { name: "Endre status" }).click();
  await expect(review).toContainText("Statusoppgåva er sett i kø i Review");
  expect(startBodies).toHaveLength(1);
  await change.locator(".sp-work-item-task-summary").click();
  await change.getByRole("combobox", { name: "Ny status" }).selectOption("in_development");
  await change.getByRole("textbox", { name: "Internt notat" }).fill("Intern plan");
  await change.getByRole("textbox", { name: /Tilbakemelding til innmeldaren/ }).fill("Vi arbeider med saka");
  await change.getByRole("button", { name: "Lagre status" }).click();
  await expect(change).toContainText("mellombels utilgjengeleg");
  await expect(change.getByRole("textbox", { name: "Internt notat" })).toHaveValue("Intern plan");
  await change.getByRole("button", { name: "Prøv same status igjen" }).click();
  expect(statusBodies).toHaveLength(2);
  expect(statusBodies[0]).toEqual(statusBodies[1]);
  await expect(publicCard).toContainText("Skrivefelt på mobil");
  await publicCard.locator(".sp-work-item-task-summary").click();
  await expect(publicCard).toContainText("Vi arbeider med saka");
  await expect(publicCard).not.toContainText("Intern plan");
  await expect(hidden).toHaveText("Statusoppdatering til innmeldaren");
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
});
