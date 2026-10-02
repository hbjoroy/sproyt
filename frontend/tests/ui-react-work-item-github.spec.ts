import { expect, test } from "@playwright/test";

const taskId = "c63ac052-a05a-4b5d-bfff-04429338df90";

test("compact GitHub task shows the public destination and retries the exact approved text", async ({ page }) => {
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
  let fail = true;
  await page.route(/\/api\/v1\/work-item-tasks\//, route => {
    const request = route.request();
    const body = request.method() === "POST" ? request.postDataJSON() as Record<string, unknown> : null;
    if (body) {
      submissions.push(body);
      if (fail) { fail = false; return route.fulfill({ status: 503, body: "Tenesta er mellombels utilgjengeleg." }); }
    }
    const messageId = body ? String(body.message_id) : new URL(request.url()).searchParams.get("message_id");
    return route.fulfill({ json: {
      id: taskId, message_id: messageId, work_item_id: taskId, revision: submissions.length > 1 ? 2 : 1,
      application_name: "Sprøyt", title: "Skrivefelt på mobil", description: "Feltet forsvinn i Edge",
      status: submissions.length > 1 ? "completed" : "pending", process_status: "waiting", delivery_status: "ready",
      category: null, priority: null, decision_status: null, node_id: "publish-github", can_request_information: false,
      information_request: null, information_response: null, assignee_name: "Harald",
      can_decide: submissions.length < 2, blocked: false,
      github_export: { repository: "sproyt/public", repository_id: 42, binding_revision: 3, can_publish: true, status: submissions.length > 1 ? "pending" : "ready",
        issue_url: null, title: "Skrivefelt på mobil", body: "Feltet forsvinn i Edge" }
    } });
  });
  await page.goto("/?participant=work-item-github-test&ui=react");
  const preview = page.locator("#sproyt-react-preview");
  const input = preview.getByRole("textbox", { name: "Skriv melding" });
  await expect(input).toBeEnabled({ timeout: 15000 });
  const name = `Github-${Date.now()}`;
  await page.evaluate(name => (window as unknown as { testSocket: WebSocket }).testSocket.send(JSON.stringify({
    protocol: "sproyt.chat.v1", type: "create_channel", request_id: crypto.randomUUID(),
    payload: { name, slug: name.toLowerCase(), kind: "public" }
  })), name);
  await expect(preview.getByRole("region", { name: `# ${name}`, exact: true })).toBeVisible();
  await input.fill(`[[work-item-task:${taskId}]]`);
  await preview.getByRole("button", { name: /^Send / }).click();
  const card = preview.locator(".sp-work-item-task");
  await expect(card).toHaveCount(1);
  await expect(card.locator("form")).toHaveCount(0);
  await card.locator(".sp-work-item-task-summary").click();
  await expect(card).toContainText("sproyt/public");
  await expect(card).toContainText("offentlege i GitHub");
  const title = card.getByRole("textbox", { name: "Tittel" });
  const body = card.getByRole("textbox", { name: "Tekst" });
  await title.fill("Skrivefeltet manglar på mobil");
  await body.fill("Steg: opne Edge og skriv ei melding.");
  await card.getByRole("button", { name: "Send til GitHub", exact: true }).click();
  await expect(card).toContainText("mellombels utilgjengeleg");
  await expect(title).toHaveValue("Skrivefeltet manglar på mobil");
  await expect(body).toHaveValue("Steg: opne Edge og skriv ei melding.");
  await expect(title).toBeDisabled();
  await card.getByRole("button", { name: "Prøv same innsending igjen" }).click();
  await expect(card).toContainText("Ventar på GitHub-innsending");
  expect(submissions).toHaveLength(2);
  expect(submissions[0]).toEqual(submissions[1]);
  expect(submissions[0]?.title).toBe("Skrivefeltet manglar på mobil");
  expect(submissions[0]?.expected_repository_id).toBe(42);
  expect(submissions[0]?.expected_binding_revision).toBe(3);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
});
