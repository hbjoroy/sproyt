import { expect, test } from "@playwright/test";

for (const newerDraft of ["", "Ei ny melding"]) {
  test(`rejected image prompt remains editable with ${newerDraft ? "a newer draft" : "an empty composer"}`, async ({ page }) => {
    let channel = "";
    let declined = false;
    await page.route(/\/api\/v1\/imagegen(?:\?.*)?$/, async route => {
      if (route.request().method() === "POST") channel = route.request().postDataJSON().channel_id;
      await route.fulfill({ json: { enabled: true, jobs: channel && !declined ? [{
        id: "react-declined", channel_id: channel, state: "ready", prompt: "Seascape", mode: "literal", error: null
      }] : [] } });
    });
    await page.route(/\/api\/v1\/imagegen\/react-declined\/preview/, route => route.fulfill({ status: 204 }));
    await page.route(/\/api\/v1\/imagegen\/react-declined\/review/, async route => {
      declined = true; await route.fulfill({ json: {} });
    });
    await page.goto(`/?participant=react-image-decline-${newerDraft ? "newer" : "empty"}`);
    const app = page.locator("#sproyt-react-preview");
    const composer = app.getByRole("textbox", { name: "Skriv melding", exact: true });
    await expect(composer).toBeEnabled();
    const command = '/imagegen literal "Seascape"';
    await composer.fill(command);
    await composer.press("Enter");
    await expect(composer).toHaveValue("");
    if (newerDraft) await composer.fill(newerDraft);
    await app.getByRole("button", { name: "Avslå", exact: true }).click();
    await expect(composer).toHaveValue(newerDraft || command);
    expect(declined).toBe(true);
    if (newerDraft) {
      const retained = app.getByRole("textbox", { name: "Prompt til nytt forsøk", exact: true });
      await expect(retained).toHaveValue(command);
      await retained.fill('/imagegen literal "A calmer seascape"');
      await expect(composer).toHaveValue(newerDraft);
    }
  });
}
