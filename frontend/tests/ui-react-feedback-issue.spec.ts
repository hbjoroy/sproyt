import { expect, test } from "@playwright/test";
import { readFile } from "node:fs/promises";

test("source message opens the published issue and retries the owner's exact assessment", async ({ page }) => {
  const bundle = await readFile(new URL("../dist/app.js", import.meta.url));
  if (process.env.SPROYT_E2E_EMBEDDED_CLIENT !== "1") {
    await page.route(/\/assets\/app\/[^/]+\/app.js$/, route => route.fulfill({ contentType: "text/javascript", body: bundle }));
  }
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
  const sourceText = "Eg vil finne att meldinga frå i går.";
  await input.fill(sourceText);
  await app.getByRole("button", { name: /^Send / }).click();
  const row = app.locator("[data-message-id]").filter({ hasText: sourceText }).first();
  await expect(row).toBeVisible();
  expect(reads).toHaveLength(0);
  const messageId = await row.getAttribute("data-message-id");
  await row.getByRole("button", { name: "Fleire meldingsval" }).click();
  await app.getByRole("button", { name: "GitHub-sak #42" }).click();
  expect(reads).toHaveLength(1);
  expect(reads[0]).toContain(`/messages/${messageId}/feedback-issue`);
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
