import { expect, test, type Page } from "@playwright/test";

async function sendReactMessage(page: Page, participant: string, body: string) {
  await page.goto(`/?participant=${participant}&ui=react`);
  const preview = page.locator("#sproyt-react-preview");
  const composer = preview.getByRole("textbox", { name: "Skriv melding" });
  await expect(composer).toBeEnabled({ timeout: 15_000 });
  await composer.fill(body);
  await composer.press("Enter");
  return preview;
}

test("React renders GFM strikethrough and tables as semantic message content", async ({ page }) => {
  const heading = `GFM Playwright ${Date.now()}`;
  const body = `${heading}\n\n~~avlyst~~\n\n| Namn | Status |\n| --- | --- |\n| Ada | Klar |\n| Bård | Ventar |`;
  const preview = await sendReactMessage(page, "react-markdown-gfm", body);
  const message = preview.locator("[data-message-id]").filter({ hasText: heading });

  await expect(message).toBeVisible();
  await expect(message.locator("del")).toHaveText("avlyst");
  const table = message.locator("table");
  await expect(table).toBeVisible();
  await expect(table.locator("th")).toHaveText(["Namn", "Status"]);
  await expect(table.locator("td")).toHaveText(["Ada", "Klar", "Bård", "Ventar"]);
});

test("React keeps raw HTML and scripts in a message inert", async ({ page }) => {
  await page.addInitScript(() => {
    (window as Window & { rawHtmlExecuted?: boolean }).rawHtmlExecuted = false;
  });
  const marker = `rå-html-${Date.now()}`;
  const body = `${marker} <img src=x onerror="window.rawHtmlExecuted = true"> <script>window.rawHtmlExecuted = true</script>`;
  const preview = await sendReactMessage(page, "react-markdown-raw-html", body);
  const message = preview.locator("[data-message-id]").filter({ hasText: marker });

  await expect(message).toBeVisible();
  await expect(message).toContainText("<script>window.rawHtmlExecuted = true</script>");
  await expect(message.locator("script, img[onerror]")).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => (window as Window & { rawHtmlExecuted?: boolean }).rawHtmlExecuted)).toBe(false);
});

test("React renders Mermaid from the bundled package without a CDN request", async ({ page }) => {
  const externalMermaidRequests: string[] = [];
  page.on("request", request => {
    if (request.url().includes("cdn.jsdelivr.net")) externalMermaidRequests.push(request.url());
  });
  const marker = `Mermaid Playwright ${Date.now()}`;
  const body = `${marker}\n\n\`\`\`mermaid\ngraph LR\n  A[Start] --> B[Ferdig]\n\`\`\``;
  const preview = await sendReactMessage(page, "react-markdown-mermaid", body);
  const message = preview.locator("[data-message-id]").filter({ hasText: marker });

  await expect(message).toBeVisible();
  await expect(message.locator(".mermaid-shell svg")).toBeVisible({ timeout: 15_000 });
  expect(externalMermaidRequests).toEqual([]);
});
