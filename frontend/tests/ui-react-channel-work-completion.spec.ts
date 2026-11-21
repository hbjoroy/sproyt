import { expect, test } from "@playwright/test";
import { createChannelViaUi } from "./support/channel-setup";

const taskId = "c63ac052-a05a-4b5d-bfff-04429338df90";
const messageId = "15292fae-20de-4e9f-96b1-c3af5ae8596f";
const nextTaskId = "c63ac052-a05a-4b5d-bfff-04429338df91";
const nextMessageId = "15292fae-20de-4e9f-96b1-c3af5ae85970";

for (const kind of ["process_pilot", "work_item"] as const) {
  test(`${kind} can be completed from channel work with a queued receipt`, async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    const participant = `channel-work-${kind}`;
    const title = kind === "process_pilot" ? "Stadfest oppsettet" : "Vurder feilmeldinga";
    let state: "ready" | "pending" | "completed" = "ready";
    let summaryRequests = 0;
    const mutations: Record<string, unknown>[] = [];
    const task = (next = false) => {
      const current = next ? "ready" : state;
      const common = {
        id: next ? nextTaskId : taskId, message_id: next ? nextMessageId : messageId,
        title: next ? "Neste oppgåve" : title, status: current === "completed" ? "completed" : "pending",
        delivery_status: current === "pending" ? "pending" : "ready", process_status: "waiting", assignee_name: "Testbrukar",
      };
      return kind === "process_pilot"
        ? { ...common, instance_id: "instance-1", node_id: "first", assignee_id: participant, can_complete: current === "ready" }
        : { ...common, work_item_id: taskId, revision: 1, application_name: "Sprøyt",
          description: "Rapporten inneheld ei konkret feilskildring.", category: null, priority: null, decision_status: null,
          node_id: "review", can_decide: current === "ready", blocked: false, can_request_information: true,
          information_request: null, information_response: null };
    };

    await page.route(/\/api\/v1\/me\/process-tasks\?/, async route => {
      summaryRequests++;
      const url = new URL(route.request().url());
      const channel = url.searchParams.get("channel_id");
      expect(channel).toMatch(/^[0-9a-f-]{36}$/);
      expect(await route.request().headerValue("x-sproyt-account")).toBeTruthy();
      const origin = { kind, channel_id: channel, channel_name: "Arbeidskanalen", circle_id: null, circle_name: null };
      const tasks = state === "ready" ? [{ ...origin, task: task() }] : [];
      return route.fulfill({ json: { next_cursor: null, tasks: [...tasks, { ...origin, task: task(true) }] } });
    });
    await page.route(/\/api\/v1\/channels\/[^/]+\/processes\?/, route => {
      return route.fulfill({ json: { processes: [], next_cursor: null } });
    });
    await page.route(/\/api\/v1\/(process-pilot\/tasks|work-item-tasks)\//, route => {
      const request = route.request();
      const url = new URL(request.url());
      const next = url.pathname.endsWith(nextTaskId);
      if (request.method() === "POST") {
        expect(url.pathname.endsWith(kind === "process_pilot" ? "/complete" : "/decide")).toBe(true);
        const body = request.postDataJSON() as Record<string, unknown>;
        expect(body.message_id).toBe(messageId);
        expect(body.request_id).toMatch(/^[0-9a-f-]{36}$/);
        if (kind === "work_item") {
          expect(body.expected_revision).toBe(1);
          expect(body.category).toBe("change");
        }
        mutations.push(body);
        state = "pending";
      } else {
        expect(url.pathname.endsWith(next ? nextTaskId : taskId)).toBe(true);
        expect(url.searchParams.get("message_id")).toBe(next ? nextMessageId : messageId);
      }
      return route.fulfill({ json: task(next) });
    });

    await page.goto(`/?participant=${participant}&ui=react`);
    const app = page.locator("#sproyt-react-preview");
    await expect(app.getByRole("textbox", { name: "Skriv melding" })).toBeEnabled({ timeout: 15_000 });
    await createChannelViaUi(page, `Arbeid-${kind}-${Date.now()}`);
    await app.getByRole("tab", { name: "Arbeid", exact: true }).click();

    const tasks = app.getByRole("region", { name: "Mine oppgåver", exact: true });
    const row = tasks.locator("details").filter({ hasText: title });
    await row.locator("summary").focus();
    await page.keyboard.press("Enter");
    const actionLabel = kind === "process_pilot" ? "Fullfør oppgåva" : "Lagre avgjerd";
    if (kind === "work_item") await row.getByRole("combobox", { name: "Kategori", exact: true }).selectOption("change");
    await row.getByRole("button", { name: actionLabel, exact: true }).click();
    await expect(row).toContainText(kind === "process_pilot" ? "Ventar på stadfesting" : "Innsending lagra");
    await expect(tasks.getByText(`${title} er fullført.`, { exact: true })).toHaveCount(0);
    expect(mutations).toHaveLength(1);

    state = "completed";
    await expect(tasks.getByRole("status").filter({ hasText: `${title} er fullført.` })).toBeVisible({ timeout: 12_000 });
    await expect(row).toHaveCount(0);
    const nextRow = tasks.locator("details").filter({ hasText: "Neste oppgåve" });
    await expect(nextRow.locator("summary")).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(nextRow.getByRole("button", { name: actionLabel, exact: true })).toBeEnabled();
    await expect.poll(() => summaryRequests).toBe(2);
    expect(mutations).toHaveLength(1);
  });
}
