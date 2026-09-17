import { expect, test } from "@playwright/test";

test("React preview changes channel notifications with host pending and local retry", async ({ page }) => {
  let releaseFailure = () => {};
  const failureGate = new Promise<void>(resolve => { releaseFailure = resolve; });
  const methods: string[] = [];
  let attempts = 0;
  await page.route("**/api/v1/channels/*/notifications**", async route => {
    methods.push(route.request().method());
    attempts++;
    if (attempts === 1) {
      await failureGate;
      await route.fulfill({ status: 503, contentType: "text/plain", body: "Mellombels utilgjengeleg" });
      return;
    }
    await route.continue();
  });

  await page.goto("/?participant=playwright-preview-channel-notifications&ui=react", { waitUntil: "domcontentloaded" });
  const preview = page.locator("#sproyt-react-preview");
  await expect(page.locator("#status")).toHaveText(/Tilkopla/, { timeout: 15_000 });
  const toggle = preview.getByRole("button", { name: /Slå (?:på|av) varsel for general/i });
  await expect(toggle).toBeVisible();
  const initial = await toggle.getAttribute("aria-pressed");
  const expectedMethod = initial === "true" ? "DELETE" : "PUT";

  await toggle.click();
  await expect(toggle).toBeDisabled();
  await expect(toggle).toHaveAttribute("aria-busy", "true");
  releaseFailure();
  const error = preview.getByRole("alert").filter({ hasText: "Kunne ikkje endre kanalvarsel" });
  await expect(error).toContainText("Mellombels utilgjengeleg");
  await expect(toggle).toHaveAttribute("aria-pressed", initial ?? "false");
  await expect(toggle).toBeEnabled();

  await error.getByRole("button", { name: "Prøv igjen" }).click();
  await expect(error).toHaveCount(0);
  await expect(toggle).toHaveAttribute("aria-pressed", initial === "true" ? "false" : "true");
  expect(methods).toEqual([expectedMethod, expectedMethod]);
});
