import { expect, test } from "@playwright/test";
import { readFile } from "node:fs/promises";

test("source message opens the published issue and retries the owner's exact assessment", async ({ page }) => {
  const bundle = await readFile(new URL("../dist/app.js", import.meta.url));
  if (process.env.SPROYT_E2E_EMBEDDED_CLIENT !== "1") {
    await page.route(/\/assets\/app\/[^/]+\/app.js$/, route => route.fulfill({ contentType: "text/javascript", body: bundle }));
  }
  await page.route(/\/feedback-issue\/intake(?:\?|$)/, route => route.fulfill({ json: null }));
  const reads: string[] = [];
  const submissions: unknown[] = [];
  let resolved = false;
  const issue = {
    issue_number: 42, issue_url: "https://github.com/hbjoroy/sproyt/issues/42", published_title: "Finne att meldinga",
    published_body: "Som lesar vil eg kunne finne att meldinga.", publication_kind: "create", stage: "review",
    github_state: "open", github_proposed: true, heart_instance_id: "05c73cf2-ec43-44ea-9734-4d065ebbeac5", can_review: true
  };
  await page.route(/\/messages\/[^/]+\/feedback-issue(?:\/review)?(?:\?|$)/, route => {
    const request = route.request();
    if (request.method() === "GET") {
      reads.push(new URL(request.url()).pathname);
      return route.fulfill({ json: resolved ? { ...issue, stage: "resolved", github_state: "closed", can_review: false } : issue });
    }
    submissions.push(request.postDataJSON());
    if (submissions.length === 1) return route.fulfill({ status: 503, body: "Vurderinga kunne ikkje stadfestast." });
    return route.fulfill({ json: { ...issue, stage: "waiting_for_close", can_review: false } });
  });
  await page.goto("/?participant=feedback-owner&ui=react");
  const app = page.locator("#sproyt-react-preview");
  const input = app.getByRole("textbox", { name: "Skriv melding", exact: true });
  await expect(input).toBeEnabled({ timeout: 15000 });
  const sourceText = `Eg vil finne att meldinga frå i går. ${crypto.randomUUID()}`;
  await input.fill(sourceText);
  await app.getByRole("button", { name: /^Send / }).click();
  const row = app.locator("[data-message-id]").filter({ hasText: sourceText }).first();
  await expect(row).toBeVisible();
  const messageId = await row.getAttribute("data-message-id");
  const sourceReads = () => reads.filter(path => path.endsWith(`/messages/${messageId}/feedback-issue`));
  expect(sourceReads()).toHaveLength(0);
  await row.getByRole("button", { name: "Legg til reaksjon", exact: true }).click();
  const picker = app.getByRole("dialog", { name: "Reager på meldinga" });
  await picker.locator("summary", { hasText: "Eigen emoji" }).click();
  await picker.getByRole("textbox", { name: "Lim inn Unicode-emoji" }).fill("📋");
  await picker.getByRole("button", { name: "Bruk emoji" }).click();
  const shortcut = row.getByRole("link", { name: "Opne sak #42 i GitHub" });
  await expect(shortcut).toHaveAttribute("href", issue.issue_url);
  await expect(shortcut).toHaveAttribute("target", "_blank");
  await page.reload();
  await expect(shortcut).toBeVisible();
  const beforeMenu = sourceReads().length;
  await row.getByRole("button", { name: "Fleire meldingsval" }).click();
  await app.getByRole("button", { name: "GitHub-sak #42" }).click();
  expect(sourceReads()).toHaveLength(beforeMenu + 1);
  const dialog = app.getByRole("dialog", { name: "GitHub-sak #42" });
  await expect(dialog.getByRole("link", { name: "Opne sak #42 i GitHub" })).toHaveAttribute("href", issue.issue_url);
  await expect(dialog.getByText("Føreslått", { exact: true })).toBeVisible();
  await dialog.getByText("Publisert sakstekst", { exact: true }).click();
  await expect(dialog.getByText(issue.published_body)).toBeVisible();
  await expect(dialog.getByRole("button", { name: "Utført" })).toBeDisabled();
  const review = "Behovet er stadfesta og kan prioriterast.";
  await dialog.getByLabel("Produkteigarvurdering").fill(review);
  await dialog.getByRole("button", { name: "Utført" }).click();
  await expect(dialog.getByLabel("Produkteigarvurdering")).toHaveValue(review);
  await dialog.getByRole("button", { name: "Prøv same vurdering igjen" }).click();
  await expect(dialog.getByText("Ventar på lukking i GitHub")).toBeVisible();
  await expect(dialog.getByLabel("Produkteigarvurdering")).toHaveCount(0);
  expect(submissions).toHaveLength(2);
  expect(submissions[0]).toEqual({ assessment: review, request_id: expect.any(String) });
  expect(submissions[1]).toEqual(submissions[0]);
  resolved = true;
  await dialog.getByRole("button", { name: "Oppdater saksstatus" }).click();
  await expect(dialog.getByText("Løyst", { exact: true })).toBeVisible();
  await expect(dialog.getByText("Lukka", { exact: true })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
});

test("the product owner can activate a selected older message once", async ({ page }) => {
  const bundle = await readFile(new URL("../dist/app.js", import.meta.url));
  if (process.env.SPROYT_E2E_EMBEDDED_CLIENT !== "1") {
    await page.route(/\/assets\/app\/[^/]+\/app.js$/, route => route.fulfill({ contentType: "text/javascript", body: bundle }));
  }
  await page.route(/\/feedback-issue\/intake(?:\?|$)/, route => route.fulfill({ json: null }));
  let activated = 0;
  await page.route(/\/feedback-issue(?:\?|$)/, route => route.fulfill({json:null}));
  await page.route(/\/feedback-issue\/activation(?:\?|$)/, route => {
    if (route.request().method() === "POST") activated++;
    return route.fulfill({ json: activated ? "pending" : "available" });
  });
  await page.goto("/?participant=feedback-owner&ui=react");
  const app = page.locator("#sproyt-react-preview");
  const source = "Tidlegare melding for vurdering";
  await app.getByRole("textbox", { name: "Skriv melding", exact: true }).fill(source);
  await app.getByRole("button", { name: /^Send / }).click();
  const menu = app.locator("[data-message-id]").filter({hasText:source}).first().getByRole("button",{name:"Fleire meldingsval"});
  await menu.click();
  await app.getByRole("button",{name:"Aktiver tilbakemeldinga"}).click();
  await expect(app.getByText("Meldinga er i kø for vurdering")).toBeVisible();
  await page.keyboard.press("Escape");
  await menu.click();
  await expect(app.getByText("Meldinga er i kø for vurdering")).toBeVisible();
  expect(activated).toBe(1);
});

test("owner can clarify a pre-publication message from their task list and inspect the same process from the message", async ({ page }) => {
  const bundle = await readFile(new URL("../dist/app.js", import.meta.url));
  if (process.env.SPROYT_E2E_EMBEDDED_CLIENT !== "1") {
    await page.route(/\/assets\/app\/[^/]+\/app.js$/, route => route.fulfill({ contentType: "text/javascript", body: bundle }));
  }
  const source = "Skjermlesaren bør få beskjed når bileta er ferdig lasta.";
  const taskId = "26c625f8-2e5a-468d-96b8-8e298541ea0b";
  const intake = { stage: "clarify", can_clarify: true, can_review: false, source_body: source,
    heart_instance_id: "80030ae5-0f96-45da-841f-761402892539", clarification_task_id: taskId, review_task_id: null,
    repository_name: "example/app", reason: "To opne saker kan gjelde denne meldinga.", candidates: [{number: 27, title: "Bilete med skjermlesar"}] };
  let selected = false;
  const sent: unknown[] = [];
  const next = { ...intake, can_clarify: false, clarification_task_id: null, reason: null, candidates: [], stage: "comment" };
  await page.route(/\/feedback-issue(?:\?|$)/, route => route.fulfill({ json: null }));
  await page.route(/\/feedback-issue\/intake(?:\?|$)/, route => route.fulfill({ json: selected ? next : intake }));
  await page.route(/\/feedback-issue\/intake\/clarification(?:\?|$)/, route => {
    sent.push(route.request().postDataJSON());
    if (sent.length === 1) return route.fulfill({ status: 503 });
    selected = true;
    return route.fulfill({ json: next });
  });
  await page.goto("/?participant=feedback-clarify&ui=react");
  const app = page.locator("#sproyt-react-preview");
  const composer = app.getByRole("textbox", { name: "Skriv melding", exact: true });
  await composer.fill(source);
  await app.getByRole("button", { name: /^Send / }).click();
  const row = app.locator("[data-message-id]").filter({ hasText: source }).first();
  const sourceId = await row.getAttribute("data-message-id");
  await page.route(/\/me\/process-tasks/, route => {
    const channel = new URL(route.request().url()).searchParams.get("channel_id");
    return route.fulfill({ json: { tasks: selected ? [] : [{ kind: "feedback", circle_id: null, circle_name: null,
      channel_id: channel, channel_name: "Felles", task: {id: taskId, message_id: sourceId, title: "Avklar tilbakemeldinga"} }], next_cursor: null } });
  });
  const work = app.getByRole("tab", { name: "Arbeid", exact: true });
  await work.click();
  const item = app.getByText("Avklar tilbakemeldinga", { exact: true });
  await item.click();
  await app.getByRole("button", { name: "Opne oppgåva" }).click();
  const dialog = app.getByRole("dialog", { name: "Tilbakemeldingsprosess" });
  await expect(dialog.getByText(source, { exact: true })).toBeVisible();
  await expect(dialog.getByText(intake.reason)).toBeVisible();
  await expect(dialog.getByRole("link", { name: "Opne sak #27 i GitHub" })).toHaveAttribute("href", "https://github.com/example/app/issues/27");
  await expect(dialog.getByLabel("Avgjerd")).toHaveValue("");
  await expect(dialog.getByRole("button", { name: "Registrer avklaring" })).toBeDisabled();
  await dialog.getByLabel("Avgjerd").selectOption("existing");
  await dialog.getByRole("button", { name: "#27 Bilete med skjermlesar" }).click();
  const issueNumber = dialog.getByLabel("Saksnummer i GitHub");
  await expect(issueNumber).toHaveValue("27");
  await issueNumber.fill("53");
  await dialog.getByLabel("Grunngjeving").fill("Gjeld same melding med skjermlesar.");
  await dialog.getByRole("button", { name: "Registrer avklaring" }).click();
  await expect(dialog.getByLabel("Avgjerd")).toBeDisabled();
  await dialog.getByRole("button", { name: "Prøv same avklaring igjen" }).click();
  await expect(dialog).toHaveCount(0);
  expect(sent).toEqual([1, 2].map(() => ({task_id: taskId, decision: "existing", issue_number: 53, note: "Gjeld same melding med skjermlesar."})));
  await work.focus();
  await page.keyboard.press("Home");
  await row.getByRole("button", { name: "Fleire meldingsval" }).click();
  await app.getByRole("button", { name: "Tilbakemeldingsprosess" }).click();
  await expect(dialog.getByText("Legg til kommentar i GitHub")).toBeVisible();
  await expect(dialog.getByLabel("Avgjerd")).toHaveCount(0);
});

test("four carried-over clarifications remain distinct in channel processes and an assigned intake review completes the exact Heart task", async ({ page }) => {
  const bundle = await readFile(new URL("../dist/app.js", import.meta.url));
  if (process.env.SPROYT_E2E_EMBEDDED_CLIENT !== "1") {
    await page.route(/\/assets\/app\/[^/]+\/app.js$/, route => route.fulfill({ contentType: "text/javascript", body: bundle }));
  }
  const messages = ["Manglande namn på biletet", "Kan ikkje bla tilbake", "Finn ikkje varslingane", "Liten skrift i kanalen", "Hugs valt tekststorleik"];
  const ids = messages.map((_, index) => `d8ad0aa1-41e8-4e7c-b8ef-c04dd7b6140${index}`);
  const tasks = messages.map((_, index) => `7ac78ce7-69f3-4a8f-987e-b71ac7bb730${index}`);
  let reviewed = false;
  const reviews: unknown[] = [];
  const issue = { issue_number: 87, issue_url: "https://github.com/example/app/issues/87", publication_kind: "create",
    published_title: messages[4], published_body: "Hugs det siste valet.", can_review: true, stage: "review",
    heart_instance_id: "d0b13d63-e6b8-444c-9459-13d6462fd639", github_state: "open" };
  const accepted = { ...issue, can_review: false, stage: "review_submitted" };
  const processRow = (channel: string, index: number) => ({
    id: "feedback:" + ids[index], kind: "feedback", channel_id: channel, channel_name: "Tilbakemeldingar",
    title: index === 4 ? "GitHub #87" : "Tilbakemelding: " + messages[index], status: index === 4 ? "review" : "clarify",
    projection: true, definition_name: "feedback-intake", definition_namespace: "sproyt", definition_version: "1.0.0", runtime: "v2",
    steps: [], history: [], steps_truncated: false, history_truncated: false, remaining_steps: null, definition_steps: [], active_node_ids: [],
  });
  await page.route(/\/channels\/[^/]+\/processes(?:\/[^?]+)?(?:\?|$)/, route => {
    const parts = decodeURIComponent(new URL(route.request().url()).pathname).split("/");
    const channel = parts[4]!;
    const key = parts[6];
    return route.fulfill({ json: key ? processRow(channel, ids.indexOf(key.slice("feedback:".length)))
      : { processes: messages.map((_, index) => processRow(channel, index)), next_cursor: null } });
  });
  await page.route(/\/me\/process-tasks/, route => {
    const channel = new URL(route.request().url()).searchParams.get("channel_id");
    return route.fulfill({ json: { tasks: messages.flatMap((text, index) => reviewed && index === 4 ? [] : [{
      kind: "feedback", circle_id: null, circle_name: null, channel_id: channel, channel_name: "Tilbakemeldingar",
      task: { id: tasks[index], message_id: ids[index], source_excerpt: text, issue_url: index === 4 ? issue.issue_url : null, title: index === 4 ? "Vurder GitHub-saka" : "Avklar tilbakemelding" },
    }]), next_cursor: null } });
  });
  await page.route(/\/feedback-issue(?:\/intake|\/review)?(?:\?|$)/, route => {
    const path = new URL(route.request().url()).pathname;
    const index = ids.findIndex(id => path.includes(id));
    if (path.endsWith("/review")) {
      reviews.push(route.request().postDataJSON()); reviewed = true;
      return route.fulfill({ json: accepted });
    }
    if (index < 0) return route.fulfill({ json: null });
    if (!path.endsWith("/intake")) return route.fulfill({ json: index === 4 ? (reviewed ? accepted : issue) : null });
    const clarify = index < 4;
    return route.fulfill({ json: {
      stage: clarify ? "clarify" : "review", source_body: messages[index], heart_instance_id: issue.heart_instance_id,
      reason: clarify ? "Meldinga venta på manuell avklaring før overgangen til Heart." : null,
      candidates: [], dismiss_only: index === 0, can_clarify: clarify, clarification_task_id: clarify ? tasks[index] : null,
      can_review: !clarify && !reviewed, review_task_id: !clarify && !reviewed ? tasks[index] : null,
    } });
  });
  await page.goto("/?participant=feedback-migrated&ui=react");
  const app = page.locator("#sproyt-react-preview");
  await app.getByRole("tab", { name: "Arbeid", exact: true }).click();
  const assigned = app.getByRole("region", { name: "Mine oppgåver" });
  const processes = app.getByRole("region", { name: "Prosessar i kanalen" });
  await expect(assigned.getByText("Avklar tilbakemelding", { exact: true })).toHaveCount(4);
  await expect(assigned.getByText(messages[2]!, { exact: true })).toBeVisible();
  const third = processes.locator("details").filter({ hasText: "Tilbakemelding: Finn ikkje varslingane" });
  await third.locator("summary").click();
  await third.getByRole("button", { name: "Opne tilbakemeldinga" }).click();
  const clarification = app.getByRole("dialog", { name: "Tilbakemeldingsprosess" });
  await expect(clarification.getByText(messages[2]!, { exact: true })).toBeVisible();
  await clarification.getByLabel("Avgjerd").selectOption("existing");
  await expect(clarification.getByLabel("Saksnummer i GitHub")).toBeEnabled();
  await clarification.getByRole("button", { name: "Lukk saka" }).click();
  const stale = processes.locator("details").filter({ hasText: "Tilbakemelding: Manglande namn på biletet" });
  await stale.locator("summary").click();
  await stale.getByRole("button", { name: "Opne tilbakemeldinga" }).click();
  await expect(clarification.getByText("Grunnlaget for publisering har endra seg. Du kan avslutte behandlinga her.")).toBeVisible();
  await expect(clarification.getByLabel("Avgjerd")).toHaveCount(0);
  await expect(clarification.getByRole("button", { name: "Registrer avklaring" })).toBeDisabled();
  await clarification.getByRole("button", { name: "Lukk saka" }).click();
  await expect(assigned.getByRole("link", { name: "Opne sak #87 i GitHub" })).toHaveAttribute("href", issue.issue_url);
  await expect(assigned.getByRole("link")).toHaveCount(1);
  const reviewTask = assigned.locator("details").filter({hasText:"Vurder GitHub-saka"});
  await reviewTask.locator("summary").click();
  await reviewTask.getByRole("button", { name: "Opne oppgåva" }).click();
  const dialog = app.getByRole("dialog", { name: "GitHub-sak #87" });
  await dialog.getByLabel("Produkteigarvurdering").fill("Prioritert for neste leveranse.");
  await dialog.getByRole("button", {name:"Utført"}).click();
  await expect(dialog).toHaveCount(0);
  expect(reviews).toEqual([{task_id:tasks[4],request_id:expect.any(String),assessment:"Prioritert for neste leveranse."}]);
  await expect(assigned.getByText("Avklar tilbakemelding", { exact: true })).toHaveCount(4);
});
