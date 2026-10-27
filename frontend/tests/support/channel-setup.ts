import { expect, type Page } from "@playwright/test";
import { destinationSurface } from "./destination-surface";

/** Exercise the application's command owner; raw socket sends have no creation
 * ticket and their private replies must remain ignored. Never reload a draft. */
export async function createChannelViaUi(page: Page, name: string, options: {
  kind?: "public" | "local" | "private"; circleName?: string;
} = {}): Promise<string> {
  const app = page.locator("#sproyt-react-preview");
  const previous = new URL(page.url()).searchParams.get("channel");
  const settings = app.getByRole("button", { name: "Innstillingar", exact: true });
  await settings.click();
  const menu = destinationSurface(app, "Innstillingar", true);
  const scope = options.circleName ?? "Felles";
  if (options.circleName) {
    await menu.getByRole("button", { name: "Administrer krets", exact: true }).click();
    const directory = destinationSurface(app, "Vel krets", true);
    await directory.getByRole("searchbox", { name: "Finn vennekrets", exact: true }).fill(options.circleName);
    await directory.getByRole("button", { name: options.circleName, exact: true }).click();
    await destinationSurface(app, options.circleName, true)
      .getByRole("button", { name: "Kanalar og medlemskap", exact: true }).click();
  } else {
    await menu.getByRole("searchbox", { name: "Søk i innstillingar", exact: true }).fill("Kanalar i Felles");
    await menu.getByRole("button", { name: "Kanalar i Felles", exact: true }).click();
  }
  await destinationSurface(app, `Kanalar i ${scope}`, true)
    .getByRole("button", { name: `Ny kanal i ${scope}`, exact: true }).click();
  const form = destinationSurface(app, `Ny kanal i ${scope}`, true);
  await form.getByRole("textbox", { name: "Kanalnamn", exact: true }).fill(name);
  await form.getByLabel("Kanaltype", { exact: true }).selectOption(options.kind ?? "public");
  await form.getByRole("button", { name: "Opprett kanal", exact: true }).click();
  await expect(form).toHaveCount(0);
  await expect.poll(() => {
    const url = new URL(page.url()), id = url.searchParams.get("channel");
    return !url.searchParams.has("view") && id !== previous && !!id && /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i.test(id);
  }).toBe(true);
  await expect(app.getByRole("textbox", { name: "Skriv melding", exact: true })).toBeEnabled();
  return new URL(page.url()).searchParams.get("channel")!;
}
