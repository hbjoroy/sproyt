import { expect, test } from "@playwright/test";
import { mkdir } from "node:fs/promises";
import { resolve } from "node:path";

const evidence = resolve("../.local/ui-restoration-20260917");
const prose = ["Takk for turen! Skal vi samlast til middag på laurdag?", "Gjerne! Eg kan ta med noko til dessert.",
  "Det høyrest fint ut. Eg tek med brød!", "Vi møtest klokka seks.", "Flott, vi sjåast!"];

for (const width of [320, 390, 1440]) for (const theme of ["light", "dark"]) {
  test(`editorial rhythm, growing draft and compact tools at ${width}px ${theme}`, async ({ page }) => {
    await mkdir(evidence, { recursive: true });
    await page.setViewportSize({ width, height: width === 320 ? 568 : width === 390 ? 844 : 900 });
    await page.addInitScript(mode => localStorage.setItem("sproyt.theme.v1", mode), theme);
    await page.addInitScript(() => {
      const NativeWebSocket = window.WebSocket;
      window.WebSocket = class extends NativeWebSocket {
        constructor(url: string | URL, protocols?: string | string[]) {
          super(url, protocols);
          (window as unknown as { visualSocket: WebSocket }).visualSocket = this;
        }
      };
    });
    await page.goto(`/?participant=visual-${width}-${theme}`);
    const app = page.locator("#sproyt-react-preview");
    const pane = app.locator(".sp-channel-pane");
    const field = pane.getByRole("textbox", { name: "Skriv melding" });
    await expect(field).toBeEnabled();
    await page.evaluate(name => (window as unknown as { visualSocket: WebSocket }).visualSocket.send(JSON.stringify({
      protocol: "sproyt.chat.v1", type: "create_channel", request_id: crypto.randomUUID(),
      payload: { slug: name, name: "Prat", kind: "public" }
    })), `visual-${width}-${theme}`);
    await expect(pane.getByRole("heading", { name: "# Prat", exact: true })).toBeVisible();
    for (const body of prose) { await field.fill(body); await field.press("Enter"); await expect(field).toHaveValue(""); }
    const messages = pane.locator("[data-message-id]").filter({ hasText: prose[0] });
    await expect(messages).toBeVisible();
    await expect(app.locator(".sp-main")).toHaveCSS("background-color", theme === "light" ? "rgb(250, 249, 245)" : "rgb(25, 27, 24)");
    await expect(messages.locator(".sp-message-meta")).toHaveCSS("padding", "0px");
    await expect(messages.locator(".sp-message-meta")).toHaveCSS("border-bottom-width", "0px");
    await expect(messages).not.toContainText("Sendt");
    const rule = await pane.locator('[data-message-id] + [data-message-id] .sp-message').first().evaluate(element => {
      const style = getComputedStyle(element, "::before");
      return { width: style.width, content: style.content };
    });
    expect(rule.width).toBe("56px");
    expect(rule.content).toBe('""');
    await field.blur();
    await page.screenshot({ path: `${evidence}/app-${width}-${theme}.png` });
    const resting = (await field.boundingBox())!;
    await field.fill("Eit lengre utkast som skal ha god plass til teksten.\n".repeat(9));
    await expect(pane.getByRole("toolbar", { name: "Skriveverktøy" })).toBeHidden();
    expect((await field.boundingBox())!.height).toBeGreaterThan(resting.height * 2);
    expect((await field.boundingBox())!.width).toBeGreaterThan(width < 500 ? width * .55 : 500);
    await expect(pane.getByRole("button", { name: "Send ↑" })).toBeInViewport();
    await page.screenshot({ path: `${evidence}/draft-${width}-${theme}.png` });
  });
}
