import { expect, test, type Page } from "@playwright/test";

async function sendReactMessage(page: Page, participant: string, body: string) {
  await page.goto(`/?participant=${participant}&ui=react`);
  const preview = page.locator("#sproyt-react-preview");
  const composer = preview.getByRole("textbox", { name: "Skriv melding" });
  await expect(composer).toBeEnabled({ timeout: 15_000 });
  await composer.fill(body);
  await preview.getByTitle("Send melding", { exact: true }).click();
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

test("Mermaid keeps diagram HTML handlers and javascript links inert", async ({ page }) => {
  await page.addInitScript(() => { (window as Window & { mermaidExecuted?: boolean }).mermaidExecuted = false; });
  const marker = `Mermaid sanitization ${Date.now()}`;
  const body = `${marker}\n\n\`\`\`mermaid\ngraph LR\n A["<a href='javascript:window.mermaidExecuted=true' onclick='window.mermaidExecuted=true'>Trygt</a>"] --> B[Ferdig]\n\`\`\``;
  const preview = await sendReactMessage(page, "react-markdown-mermaid-safety", body);
  const diagram = preview.locator("[data-message-id]").filter({ hasText: marker }).locator(".mermaid-shell svg");
  await expect(diagram).toBeVisible({ timeout: 15_000 });
  await expect(diagram.locator("script, [onerror], [onload], [onclick], a[href^='javascript:']")).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => (window as Window & { mermaidExecuted?: boolean }).mermaidExecuted)).toBe(false);
});

test("saved dice outcomes show two local sprites in compact light and dark chat",async({page})=>{
  await page.setViewportSize({width:390,height:800});
  const marker=`Dice preview ${Date.now()}`;
  const preview=await sendReactMessage(page,"react-dice",`${marker}\n🎲 4 + 2 = 6\n\n\`\`\`dice\n4,2\n\`\`\``);
  const message=preview.locator('[data-message-id]').filter({hasText:marker});
  const roll=message.getByRole('img',{name:'Terningkast: 4 og 2'});
  await expect(roll).toBeVisible();
  await expect(roll.locator('img')).toHaveCount(2);
  await expect.poll(()=>roll.locator('img').evaluateAll(images=>images.every(image=>image instanceof HTMLImageElement && image.complete && image.naturalWidth===256))).toBe(true);
  const bounds=await roll.boundingBox();expect(bounds?.width).toBeLessThanOrEqual(390);
  for(const colorScheme of ['light','dark'] as const) {
    await page.emulateMedia({colorScheme});
    await page.screenshot({path:`test-results/dice-${colorScheme}.png`});
  }
});


test("bar results show a compact linked table with optional detail on mobile", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 800 });
  const body = "**Barforslag · Google Maps**\n\n| # | Stad |\n| ---: | --- |\n| 1 | [Ein pub med eit svært langt namn ved jernbanestasjonen](<https://www.google.com/maps?cid=1>) |\n| 2 | [Pub B](<https://www.google.com/maps?cid=2>) |\n\nKjelde: Google Maps · 28.10.2026 17:00 UTC.\n\n**Om stadene**\n\nHer er den lengre omtalen frå kjelda.\n\nOpningstider og tilbod kan endre seg.";
  const preview = await sendReactMessage(page, "compact-bar-table", body);
  const result = preview.locator(".sp-bar-results");
  await expect(result.locator("table")).toBeVisible();
  await expect(result.locator("tbody tr")).toHaveCount(2);
  await expect(result.getByRole("link", { name: "Pub B", exact: true })).toHaveAttribute("href", "https://www.google.com/maps?cid=2");
  await expect(result.getByText("Her er den lengre omtalen frå kjelda.")).toBeHidden();
  expect((await result.boundingBox())!.height).toBeLessThan(230);
  expect(await result.evaluate(el => el.scrollWidth <= el.clientWidth)).toBe(true);
  expect(await result.locator("table").evaluate(el => el.scrollWidth <= el.clientWidth)).toBe(true);
  for (const colorScheme of ["light", "dark"] as const) {
    await page.emulateMedia({ colorScheme });
    await page.screenshot({ path: `test-results/bar-table-${colorScheme}.png` });
  }
  await result.locator("summary").click();
  await expect(result.getByText("Her er den lengre omtalen frå kjelda.")).toBeVisible();
});
