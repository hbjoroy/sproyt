import { expect, test, type Page } from "@playwright/test";
import { build } from "esbuild";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

// Keep fixture coordinates independent of the application's phone-sized projects.
test.use({ viewport: { width: 1280, height: 720 }, isMobile: false });

async function openViewer(page: Page) {
  const bundle = await build({
    stdin: {
      contents: `import { createRoot } from "react-dom/client";
        import { ImageViewer } from "./src/ui/react/image-viewer";
        const root = createRoot(document.getElementById("viewer"));
        root.render(<ImageViewer
          src="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='1600' height='1200'%3E%3Crect width='1600' height='1200' fill='teal'/%3E%3C/svg%3E"
          downloadSrc="/test-photo" name="test-photo.svg" onClose={() => root.unmount()} />);`,
      resolveDir: fileURLToPath(new URL("..", import.meta.url)), loader: "tsx"
    }, bundle: true, write: false, jsx: "automatic", format: "iife"
  });
  await page.setContent('<div id="viewer"></div>');
  await page.addStyleTag({ content: await readFile(new URL("../src/ui/react-app.css", import.meta.url), "utf8") });
  await page.addScriptTag({ content: bundle.outputFiles[0]!.text });
  await expect(page.getByRole("dialog", { name: "test-photo.svg" })).toBeVisible();
}

test("a completed pinch does not become the first half of a double tap", async ({ page, context, browserName }) => {
  test.skip(browserName !== "chromium", "Real multi-touch input uses Chromium CDP");
  await openViewer(page);
  const cdp = await context.newCDPSession(page);
  const zoom = page.getByRole("button", { name: "Tilpass biletet til skjermen" });
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: 560, y: 300, id: 1 }, { x: 680, y: 300, id: 2 }] });
  await cdp.send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ x: 500, y: 300, id: 1 }, { x: 740, y: 300, id: 2 }] });
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [{ x: 740, y: 300, id: 2 }] });
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
  await expect(zoom).toHaveText("200%");
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: 640, y: 300, id: 3 }] });
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
  await expect(zoom).toHaveText("200%");
});

test("a long stationary touch does not become the first half of a double tap", async ({ page, context, browserName }) => {
  test.skip(browserName !== "chromium", "Real multi-touch input uses Chromium CDP");
  await openViewer(page);
  const cdp = await context.newCDPSession(page);
  const zoom = page.getByRole("button", { name: "Tilpass biletet til skjermen" });
  await page.getByRole("button", { name: "Zoom inn" }).click();
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: 640, y: 300, id: 1 }] });
  await page.waitForTimeout(400);
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: 640, y: 300, id: 2 }] });
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
  await expect(zoom).toHaveText("140%");
});

test("two short touch taps toggle zoom once", async ({ page, context, browserName }) => {
  test.skip(browserName !== "chromium", "Real multi-touch input uses Chromium CDP");
  await openViewer(page);
  const cdp = await context.newCDPSession(page);
  for (const id of [1, 2]) {
    await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: 640, y: 300, id }] });
    await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
  }
  await expect(page.getByRole("button", { name: "Tilpass biletet til skjermen" })).toHaveText("250%");
});

test("pan and canceled touches leave the next tap at the current zoom", async ({ page, context, browserName }) => {
  test.skip(browserName !== "chromium", "Real multi-touch input uses Chromium CDP");
  await openViewer(page);
  const cdp = await context.newCDPSession(page);
  const zoom = page.getByRole("button", { name: "Tilpass biletet til skjermen" });
  await page.getByRole("button", { name: "Zoom inn" }).click();
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: 640, y: 300, id: 1 }] });
  await cdp.send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ x: 700, y: 360, id: 1 }] });
  await expect(page.getByRole("img")).toHaveCSS("transform", "matrix(1.4, 0, 0, 1.4, 60, 60)");
  await cdp.send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ x: 640, y: 300, id: 1 }] });
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: 640, y: 300, id: 2 }] });
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
  await expect(zoom).toHaveText("140%");
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: 640, y: 300, id: 3 }] });
  await cdp.send("Input.dispatchTouchEvent", { type: "touchCancel", touchPoints: [] });
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: 640, y: 300, id: 4 }] });
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
  await expect(zoom).toHaveText("140%");
});

test("losing pointer capture clears the gesture before the next pan", async ({ page }) => {
  await openViewer(page);
  await page.getByRole("button", { name: "Zoom inn" }).click();
  const surface = page.locator(".sp-image-viewer-surface");
  await surface.evaluate(element => element.addEventListener("pointerdown", event => {
    element.setAttribute("data-test-pointer-id", String((event as PointerEvent).pointerId));
  }));
  await page.mouse.move(640, 300);
  await page.mouse.down();
  await page.mouse.move(700, 360);
  await expect(page.getByRole("img")).toHaveCSS("transform", "matrix(1.4, 0, 0, 1.4, 60, 60)");
  await surface.evaluate(element => element.releasePointerCapture(Number(element.getAttribute("data-test-pointer-id"))));
  await page.mouse.move(800, 460);
  await page.mouse.up();
  await expect(page.getByRole("img")).toHaveCSS("transform", "matrix(1.4, 0, 0, 1.4, 60, 60)");
  await page.mouse.down();
  await page.mouse.move(740, 400);
  await page.mouse.up();
  await expect(page.getByRole("img")).toHaveCSS("transform", "matrix(1.4, 0, 0, 1.4, 0, 0)");
});

test("native image menus are suppressed only on the pan surface and controls remain usable", async ({ page }) => {
  await openViewer(page);
  await page.getByRole("button", { name: "Zoom inn" }).click();
  const surface = page.locator(".sp-image-viewer-surface");
  const imageMenuAllowed = await surface.locator("img").evaluate(element => element.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true })));
  expect(imageMenuAllowed).toBe(false);
  const download = page.getByRole("link", { name: "Last ned test-photo.svg", exact: true });
  const controlMenuAllowed = await download.evaluate(element => element.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true })));
  expect(controlMenuAllowed).toBe(true);
  await page.mouse.move(640, 300); await page.mouse.down(); await page.mouse.move(700, 360); await page.mouse.up();
  await expect(page.getByRole("img")).toHaveCSS("transform", "matrix(1.4, 0, 0, 1.4, 60, 60)");
  await page.getByRole("button", { name: "Lukk bilete" }).click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
});

test("mouse pan, double click, keyboard zoom and dismissal remain available", async ({ page }) => {
  await openViewer(page);
  const zoom = page.getByRole("button", { name: "Tilpass biletet til skjermen" });
  await page.locator(".sp-image-viewer-surface").dblclick();
  await expect(zoom).toHaveText("250%");
  await page.mouse.move(640, 300);
  await page.mouse.down();
  await page.mouse.move(740, 400, { steps: 3 });
  await page.mouse.up();
  await expect(page.getByRole("img")).toHaveCSS("transform", "matrix(2.5, 0, 0, 2.5, 100, 100)");
  await page.keyboard.press("0");
  await expect(zoom).toHaveText("100%");
  await page.keyboard.press("+");
  await expect(zoom).toHaveText("125%");
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toHaveCount(0);
});
