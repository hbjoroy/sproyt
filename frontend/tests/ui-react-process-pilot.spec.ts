import { expect, test, type Page } from "@playwright/test";

const taskId = "c63ac052-a05a-4b5d-bfff-04429338df90";
const macro = `[[process-task:${taskId}]]`;

async function pilot(page: Page, canComplete: boolean) {
  await page.addInitScript(() => {
    const NativeSocket = WebSocket;
    window.WebSocket = class extends NativeSocket {
      constructor(url: string | URL, protocols?: string | string[]) {
        super(url, protocols); (window as unknown as { testSocket: WebSocket }).testSocket = this;
      }
    };
  });
  const completions: { request_id: string; message_id: string }[] = [];
  let completed = false;
  let completionAccepted = false;
  let rejectCompletion = true;
  let reads = 0;
  let configured = false;
  const starts: string[] = [];
  await page.route(/\/api\/v1\/channels\/[^/]+\/process-pilot(?:\?.*)?$/, route => {
    if (route.request().method() === "POST") configured = true;
    return route.fulfill({ json: { configured, can_configure: canComplete, can_start: configured && canComplete, assignee_name: "Harald" } });
  });
  await page.route(/\/api\/v1\/channels\/[^/]+\/process-pilot\/start(?:\?.*)?$/, route => {
    starts.push(route.request().postDataJSON().request_id);
    return route.fulfill({ json: { id: "instance-1", status: "running" } });
  });
  await page.route(/\/api\/v1\/process-pilot\/tasks\//, route => {
    const request = route.request();
    let messageId = new URL(request.url()).searchParams.get("message_id");
    if (request.method() === "POST") {
      const body = request.postDataJSON(); completions.push(body); messageId = body.message_id;
      if (rejectCompletion) { rejectCompletion = false; return route.fulfill({ status: 503, body: "Tenesta er mellombels utilgjengeleg." }); }
      completionAccepted = true;
    } else reads++;
    return route.fulfill({ json: { id: taskId, message_id: messageId, instance_id: "instance-1", node_id: "first",
      status: completed ? "completed" : "pending", title: "Første oppgåve", assignee_id: "assignee-1", assignee_name: "Harald",
      can_complete: canComplete && !completed, delivery_status: completionAccepted && !completed ? "pending" : "ready" } });
  });
  return { completions, starts, configured: () => configured, reads: () => reads, confirmCompletion: () => { completed = true; } };
}

async function isolatedChannel(page: Page) {
  const preview = page.locator("#sproyt-react-preview");
  await expect(preview.getByRole("textbox", { name: "Skriv melding" })).toBeEnabled({ timeout: 15000 });
  const name = `Pilot-${Date.now()}`;
  await page.evaluate(name => (window as unknown as { testSocket: WebSocket }).testSocket.send(JSON.stringify({
    protocol: "sproyt.chat.v1", type: "create_channel", request_id: crypto.randomUUID(),
    payload: { name, slug: name.toLowerCase(), kind: "public" }
  })), name);
  await expect(preview.getByRole("region", { name: `# ${name}`, exact: true })).toBeVisible();
}

test("pilot requires explicit setup/start; assigned task is collapsed, retryable and preserves composer drafts", async ({ page }) => {
  const state = await pilot(page, true);
  await page.goto("/?participant=react-pilot-assignee&ui=react");
  await isolatedChannel(page);
  const preview = page.locator("#sproyt-react-preview");
  const input = preview.getByRole("textbox", { name: "Skriv melding" });
  await expect(input).toBeEnabled({ timeout: 15000 });
  expect(state.configured()).toBe(false);
  expect(state.starts).toHaveLength(0);
  await preview.getByRole("button", { name: "Kanalval", exact: true }).filter({ visible: true }).click();
  const dialog = preview.getByRole("dialog", { name: /^Kanalval:/ });
  await dialog.getByRole("button", { name: "Aktiver prosesspilot for meg" }).click();
  await expect(dialog.getByRole("button", { name: "Start testprosess", exact: true })).toBeEnabled();
  expect(state.starts).toHaveLength(0);
  await dialog.getByRole("button", { name: "Start testprosess", exact: true }).click();
  await expect(dialog).toContainText("Prosessen er starta");
  await dialog.getByRole("button", { name: "Lukk kanalvala" }).click();
  await input.fill(macro); await input.press("Enter");
  const task = preview.locator(".sp-process-task").last();
  await expect(task.locator("summary")).toContainText("Første oppgåve");
  await expect(task).not.toHaveAttribute("open", "");
  await task.locator("summary").focus(); await page.keyboard.press("Enter");
  await input.fill("Utkastet skal bli verande");
  await task.getByRole("button", { name: "Fullfør oppgåva" }).click();
  await expect(task).toContainText("mellombels utilgjengeleg");
  await expect(task.locator("summary")).toContainText("Ventar");
  await task.getByRole("button", { name: "Fullfør oppgåva" }).click();
  await expect(task.getByRole("button", { name: "Ventar på stadfesting" })).toBeDisabled();
  await expect(task).toContainText("Fullføringa er send");
  await expect(task.locator("summary")).toContainText("Ventar");
  await expect(task.locator("summary")).not.toContainText("Fullført");
  state.confirmCompletion();
  await task.getByRole("button", { name: "Hent status på nytt" }).click();
  await expect(task.locator("summary")).toContainText("Fullført");
  await expect(input).toHaveValue("Utkastet skal bli verande");
  expect(state.completions).toHaveLength(2);
  expect(state.completions[0]!.request_id).toBe(state.completions[1]!.request_id);
  expect(state.completions[0]!.message_id).toBeTruthy();
  await page.reload();
  await expect(task.locator("summary")).toContainText("Fullført", { timeout: 15000 });
  await expect(task).not.toHaveAttribute("open", "");
});

test("channel members see read-only task details and malformed task markers stay ordinary messages", async ({ page }, testInfo) => {
  const state = await pilot(page, false);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/?participant=react-pilot-reader&ui=react");
  await isolatedChannel(page);
  const preview = page.locator("#sproyt-react-preview");
  const input = preview.getByRole("textbox", { name: "Skriv melding" });
  await expect(input).toBeEnabled({ timeout: 15000 });
  await input.fill("[[process-task:not-a-uuid]]"); await input.press("Enter");
  await expect(preview.getByText("[[process-task:not-a-uuid]]", { exact: true })).toBeVisible();
  expect(state.reads()).toBe(0);
  await input.fill(macro); await input.press("Enter");
  const task = preview.locator(".sp-process-task").last();
  await expect(task.locator("summary")).toContainText("Første oppgåve");
  await task.locator("summary").click();
  await expect(task).toContainText("Tildelt Harald");
  await expect(task).toContainText("Berre personen som har fått oppgåva");
  await expect(task.getByRole("button", { name: "Fullfør oppgåva" })).toHaveCount(0);
  expect(state.completions).toHaveLength(0);
  await task.screenshot({ path: testInfo.outputPath("process-task-reader-compact.png") });
});
