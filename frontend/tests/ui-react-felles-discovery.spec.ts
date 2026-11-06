import { expect, test, type Page } from "@playwright/test";
import { createChannelViaUi } from "./support/channel-setup";
import { destinationSurface } from "./support/destination-surface";

async function enter(page: Page, participant: string) {
  await page.goto(`/?participant=${participant}&ui=react`);
  const app = page.locator("#sproyt-react-preview");
  await expect(app.getByRole("textbox", { name: "Skriv melding" })).toBeEnabled({ timeout: 15_000 });
  return app;
}

test("Finn kanalar i Felles discovers an open channel and joins it from another account", async ({ page }) => {
  test.setTimeout(60_000);
  await page.setViewportSize({ width: 390, height: 844 });
  const suffix = Date.now();
  const participant = `felles-creator-${suffix}`;
  const publicName = `Open Felles ${suffix}`;
  const privateName = `Privat Felles ${suffix}`;
  const circleName = `Krets ${suffix}`;
  const circleChannel = `Open kretskanal ${suffix}`;
  const creator = await enter(page, participant);
  const publicId = await createChannelViaUi(page, publicName);
  await createChannelViaUi(page, privateName, { kind: "private" });
  await page.goto(`/?participant=${participant}&ui=react&view=settings&section=community&task=create-circle`);
  const createCircle = destinationSurface(creator, "Ny vennekrets");
  await createCircle.getByLabel("Namn på vennekrets").fill(circleName);
  await createCircle.getByRole("button", { name: "Opprett vennekrets" }).click();
  await expect(createCircle).toHaveCount(0);
  await enter(page, participant);
  await createChannelViaUi(page, circleChannel, { circleName });

  const scopes: unknown[] = [];
  page.on("websocket", socket => socket.on("framesent", ({ payload }) => {
    const command = JSON.parse(String(payload));
    if (command.type === "list_joinable_channels") scopes.push(command.payload.circle_id);
  }));
  const viewer = await enter(page, `felles-viewer-${suffix}`);
  const openDiscovery = async () => {
    const back = viewer.getByRole("button", { name: "Samtalar", exact: true });
    if (await back.isVisible()) await back.click();
    const tab = viewer.getByRole("tab", { name: /^Kretsar/ });
    if (await tab.getAttribute("aria-selected") !== "true") await tab.click();
    const shared = viewer.locator('[data-conversation-group="scope:shared"]');
    await shared.getByRole("button", { name: "Val for Felles", exact: true }).click();
    await shared.getByRole("button", { name: "Finn kanalar i Felles", exact: true }).click();
    return destinationSurface(viewer, "Kanalar i Felles");
  };
  const channels = await openDiscovery();
  const discovery = channels.getByRole("region", { name: "Finn opne kanalar" });
  const join = discovery.getByRole("button", { name: `Bli med i ${publicName}`, exact: true });
  await expect(join).toBeVisible();
  expect(scopes).toContain(null);
  await expect(discovery).not.toContainText(privateName);
  await expect(discovery).not.toContainText(circleChannel);
  await join.click();
  await expect.poll(() => new URL(page.url()).searchParams.get("channel")).toBe(publicId);
  await expect(viewer.getByRole("textbox", { name: "Skriv melding", exact: true })).toBeEnabled();
  await page.goBack();
  await expect(channels.getByRole("button", { name: `# ${publicName} Gå inn`, exact: true })).toBeVisible();
  await expect(channels.getByText("Hentar opne kanalar …")).toHaveCount(0);
  await expect(join).toHaveCount(0);
});
