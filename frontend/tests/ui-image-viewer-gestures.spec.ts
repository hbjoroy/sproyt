import { expect, test, type Page } from "@playwright/test";
import { build } from "esbuild";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

// Keep fixture coordinates independent of the application's phone-sized projects.
test.use({ viewport: { width: 1280, height: 720 }, isMobile: false });

async function openViewer(page: Page, width = 1600, height = 1200) {
  const bundle = await build({
    stdin: {
      contents: `import { createRoot } from "react-dom/client";
        import { ImageViewer } from "./src/ui/react/image-viewer";
        const root = createRoot(document.getElementById("viewer"));
        root.render(<ImageViewer
          src="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='${width}' height='${height}'%3E%3Crect width='${width}' height='${height}' fill='teal'/%3E%3C/svg%3E"
          downloadSrc="/test-photo" name="test-photo.svg" onClose={() => root.unmount()} />);`,
      resolveDir: fileURLToPath(new URL("..", import.meta.url)), loader: "tsx"
    }, bundle: true, write: false, jsx: "automatic", format: "iife"
  });
  await page.setContent('<div id="viewer"></div>');
  await page.addStyleTag({ content: await readFile(new URL("../src/ui/react-app.css", import.meta.url), "utf8") });
  await page.addScriptTag({ content: bundle.outputFiles[0]!.text });
  await expect(page.getByRole("dialog", { name: "test-photo.svg" })).toBeVisible();
}

test("an overflowing image pans to both vertical edges at the opening zoom", async ({ page }) => {
  await openViewer(page, 1600, 1800);
  const image = page.getByRole("img");
  const surface = page.locator(".sp-image-viewer-surface");
  await expect.poll(() => image.evaluate(element => (element as HTMLImageElement).naturalWidth)).toBeGreaterThan(0);
  const before = (await image.boundingBox())!;
  const viewport = (await surface.boundingBox())!;
  expect(before.width).toBeCloseTo(viewport.width, 0);
  expect(before.height).toBeGreaterThan(viewport.height);
  await page.mouse.move(640, 360); await page.mouse.down(); await page.mouse.move(640, 460); await page.mouse.up();
  await expect.poll(async () => (await image.boundingBox())!.y - before.y).toBeCloseTo(100, 0);
  for (let i = 0; i < 3; i++) {
    await page.mouse.move(640, 600); await page.mouse.down(); await page.mouse.move(640, 100); await page.mouse.up();
  }
  const bottom = (await image.boundingBox())!;
  expect(bottom.y + bottom.height).toBeCloseTo(viewport.y + viewport.height, 0);
  for (let i = 0; i < 3; i++) {
    await page.mouse.move(640, 100); await page.mouse.down(); await page.mouse.move(640, 600); await page.mouse.up();
  }
  expect((await image.boundingBox())!.y).toBeCloseTo(viewport.y, 0);
  await page.getByRole("button", { name: "Tilbakestill biletvising" }).click();
  await expect.poll(async () => (await image.boundingBox())!.y).toBeCloseTo(before.y, 0);
});

test("one finger can pan an overflowing image without zooming first", async ({ page, context, browserName }) => {
  test.skip(browserName !== "chromium", "Real touch input uses Chromium CDP");
  await openViewer(page, 1600, 1800);
  const cdp = await context.newCDPSession(page);
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: 640, y: 360, id: 1 }] });
  await cdp.send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ x: 640, y: 460, id: 1 }] });
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
  await expect(page.getByRole("img")).toHaveCSS("transform", "matrix(1, 0, 0, 1, 0, 100)");
});

test("resize reclamps the opening pan and a fitting image cannot be dragged away", async ({ page }) => {
  await openViewer(page, 1600, 1800);
  await page.mouse.move(640, 300); await page.mouse.down(); await page.mouse.move(640, 500); await page.mouse.up();
  await expect(page.getByRole("img")).toHaveCSS("transform", "matrix(1, 0, 0, 1, 0, 200)");
  await page.setViewportSize({ width: 820, height: 1000 });
  await expect(page.getByRole("img")).toHaveCSS("transform", "matrix(1, 0, 0, 1, 0, 0)");
  const viewport = (await page.locator(".sp-image-viewer-surface").boundingBox())!;
  const image = (await page.getByRole("img").boundingBox())!;
  expect(image.y + image.height / 2).toBeCloseTo(viewport.y + viewport.height / 2, 0);
  await page.mouse.move(410, 400); await page.mouse.down(); await page.mouse.move(510, 500); await page.mouse.up();
  await expect(page.getByRole("img")).toHaveCSS("transform", "matrix(1, 0, 0, 1, 0, 0)");
});

test("resizing during a drag cancels its old baseline before the next gesture", async ({ page }) => {
  await openViewer(page, 1600, 1800);
  const image = page.getByRole("img");
  await page.mouse.move(640, 500); await page.mouse.down(); await page.mouse.move(640, 300);
  await expect(image).toHaveCSS("transform", "matrix(1, 0, 0, 1, 0, -200)");
  await page.setViewportSize({ width: 820, height: 600 });
  await expect.poll(async () => {
    const bounds = (await image.boundingBox())!;
    return bounds.y + bounds.height;
  }).toBeCloseTo(600, 0);
  const after = (await image.boundingBox())!;
  await page.mouse.move(410, 450); await page.mouse.up();
  expect((await image.boundingBox())!.y).toBeCloseTo(after.y, 0);
  await page.mouse.move(410, 300); await page.mouse.down(); await page.mouse.move(410, 350); await page.mouse.up();
  await expect.poll(async () => (await image.boundingBox())!.y - after.y).toBeCloseTo(50, 0);
});

test("pinching past zoom limits keeps the focal point at the clamped scale", async ({ page, context, browserName }) => {
  test.skip(browserName !== "chromium", "Real multi-touch input uses Chromium CDP");
  await openViewer(page);
  const cdp = await context.newCDPSession(page);
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: 730, y: 360, id: 1 }, { x: 750, y: 360, id: 2 }] });
  await cdp.send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ x: 640, y: 360, id: 1 }, { x: 840, y: 360, id: 2 }] });
  await expect(page.getByRole("img")).toHaveCSS("transform", "matrix(5, 0, 0, 5, -400, 0)");
  await cdp.send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ x: 739, y: 360, id: 1 }, { x: 741, y: 360, id: 2 }] });
  await expect(page.getByRole("img")).toHaveCSS("transform", "matrix(1, 0, 0, 1, 0, 0)");
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
});

test("a completed pinch does not become the first half of a double tap", async ({ page, context, browserName }) => {
  test.skip(browserName !== "chromium", "Real multi-touch input uses Chromium CDP");
  await openViewer(page);
  const cdp = await context.newCDPSession(page);
  const zoom = page.getByRole("button", { name: "Tilbakestill biletvising" });
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
  const zoom = page.getByRole("button", { name: "Tilbakestill biletvising" });
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
  await expect(page.getByRole("button", { name: "Tilbakestill biletvising" })).toHaveText("250%");
});

test("pan and canceled touches leave the next tap at the current zoom", async ({ page, context, browserName }) => {
  test.skip(browserName !== "chromium", "Real multi-touch input uses Chromium CDP");
  await openViewer(page);
  const cdp = await context.newCDPSession(page);
  const zoom = page.getByRole("button", { name: "Tilbakestill biletvising" });
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
  const zoom = page.getByRole("button", { name: "Tilbakestill biletvising" });
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
