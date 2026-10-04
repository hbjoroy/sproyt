import { expect, test, type Locator } from "@playwright/test";
import { build } from "esbuild";
import { fileURLToPath } from "node:url";
import { createServer, type RequestListener, type Server } from "node:http";
import { readFile } from "node:fs/promises";

async function serve(handler: RequestListener): Promise<{ server: Server; url: string }> {
  const server = createServer(handler);
  await new Promise<void>(resolve => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  if (!address || typeof address === "string") throw new Error("Missing fixture address");
  return { server, url: `http://127.0.0.1:${address.port}` };
}

test("native downloads and preview fallback keep the original app context and draft usable", async ({ page, context }) => {
  let html = "";
  const app = await serve((request, response) => {
    if (request.url === "/attachment") {
      response.writeHead(200, { "content-type": "image/svg+xml", "content-disposition": 'attachment; filename="bilete.svg"' });
      response.end('<svg xmlns="http://www.w3.org/2000/svg"/>');
    } else { response.writeHead(200, { "content-type": "text/html; charset=utf-8" }); response.end(html); }
  });
  const inline = await serve((_, response) => {
    response.writeHead(200, { "content-type": "text/html; charset=utf-8" }); response.end("<h1>Førehandsvising</h1>");
  });
  try {
    const origin = app.url;
    const bundle = await build({ stdin: { contents: `
      import { createRoot } from "react-dom/client";
      import { useState } from "react";
      import { ImageDownloadLink } from "./src/ui/react/image-download-link";
      function App() {
        const [draft, setDraft] = useState("");
        return <><label>Utkast<input value={draft} onChange={event => setDraft(event.target.value)} /></label>
          <ImageDownloadLink className="download" href="/attachment" name="bilete.svg" />
          <ImageDownloadLink className="preview" href="${inline.url}/inline-preview" name="preview.svg" /></>;
      }
      createRoot(document.getElementById("app")).render(<App />);`,
      resolveDir: fileURLToPath(new URL("..", import.meta.url)), loader: "tsx" },
      bundle: true, write: false, jsx: "automatic", format: "iife" });
    html = `<div id="app"></div><script>${bundle.outputFiles[0]!.text}</script>`;
    // Cross-origin inline responses cannot rely on the download attribute. This
    // exercises the separate context used when a browser opens a preview instead.
    await page.goto(`${origin}/fixture`);
    const draft = page.getByRole("textbox", { name: "Utkast", exact: true });
    await draft.fill("Utkastet skal stå");
    const download = page.getByRole("link", { name: "Last ned bilete.svg", exact: true });
    await expect(download).toHaveAttribute("download", "bilete.svg");
    await expect(download).toHaveAttribute("target", "_blank");
    await expect(download).toHaveAttribute("rel", "noopener noreferrer");
    const downloaded = page.waitForEvent("download");
    await download.click();
    expect((await downloaded).suggestedFilename()).toBe("bilete.svg");
    await expect(page).toHaveURL(`${origin}/fixture`);
    await expect(draft).toHaveValue("Utkastet skal stå");
    const opened = context.waitForEvent("page");
    await page.getByRole("link", { name: "Last ned preview.svg", exact: true }).click();
    const preview = await opened;
    await expect(preview.getByRole("heading", { name: "Førehandsvising" })).toBeVisible();
    expect(await preview.evaluate(() => window.opener)).toBeNull();
    await expect(page).toHaveURL(`${origin}/fixture`);
    await preview.close();
    await draft.fill("Framleis i same samtale");
    await expect(draft).toHaveValue("Framleis i same samtale");
  } finally {
    app.server.closeAllConnections(); app.server.close();
    inline.server.closeAllConnections(); inline.server.close();
  }
});

test("installed iOS prepares the original file and shares only on an explicit second gesture without navigation on cancellation or errors", async ({ page, context }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  let html = "";
  let responseKind = "image";
  const fixture = await serve((request, response) => {
    if (request.url === "/image") {
      if (responseKind === "error") { response.writeHead(403); response.end("denied"); }
      else if (responseKind === "html") { response.writeHead(200, { "content-type": "text/html" }); response.end("login"); }
      else if (responseKind === "large") { response.writeHead(200, { "content-type": "image/png", "content-length": String(35 * 1024 * 1024 + 1) }); response.flushHeaders(); response.write(Buffer.from([137, 80, 78, 71])); }
      else if (responseKind === "heic" || responseKind === "avif") { response.writeHead(200, { "content-type": `image/${responseKind}`, "cache-control": "no-store" }); response.end(Buffer.concat([Buffer.from([0, 0, 0, 24]), Buffer.from(`ftyp${responseKind}`)])); }
      else { response.writeHead(200, { "content-type": "image/png" }); response.end(Buffer.from([137, 80, 78, 71, 1, 2, 3])); }
    } else { response.writeHead(200, { "content-type": "text/html; charset=utf-8" }); response.end(html); }
  });
  try {
    const bundle = await build({ stdin: { contents: `
      import { createRoot } from "react-dom/client";
      import { useState } from "react";
      import { Theme } from "@sproyt/ui/react";
      import { ImageDownloadLink } from "./src/ui/react/image-download-link";
      import { ImageViewer } from "./src/ui/react/image-viewer";
      function App() {
        const [viewer, setViewer] = useState(false);
        return <Theme><label>Utkast<input defaultValue="bevar meg" /></label>
          <ImageDownloadLink className="sp-media-download" href="/image" name="Originalt namn.png" />
          <button onClick={() => setViewer(true)}>Vis original</button>
          {viewer && <ImageViewer src="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='600' height='400'/%3E"
            downloadSrc="/image" name="Originalt namn.png" onClose={() => setViewer(false)} />}</Theme>;
      }
      createRoot(document.getElementById("app")).render(<App />);`,
      resolveDir: fileURLToPath(new URL("..", import.meta.url)), loader: "tsx" },
      bundle: true, write: false, jsx: "automatic", format: "iife" });
    const styles = await readFile(new URL("../node_modules/@sproyt/ui/dist/styles.css", import.meta.url), "utf8")
      + await readFile(new URL("../src/ui/react-app.css", import.meta.url), "utf8");
    html = `<meta name="viewport" content="width=device-width, initial-scale=1"><style>${styles}</style><div id="app"></div><script>${bundle.outputFiles[0]!.text}</script>`;
    await page.addInitScript(() => {
      Object.defineProperty(navigator, "platform", { value: "MacIntel" });
      Object.defineProperty(navigator, "maxTouchPoints", { value: 5 });
      Object.defineProperty(navigator, "standalone", { value: true });
      const state = window as typeof window & { shareMode: string; shareCalls: any[]; canShareFiles: boolean };
      state.shareMode = "cancel"; state.shareCalls = []; state.canShareFiles = true;
      Object.defineProperty(navigator, "canShare", { value: (data: ShareData) => state.canShareFiles && data.files?.[0] instanceof File });
      Object.defineProperty(navigator, "share", { configurable: true, value: (data: ShareData) => {
        const file = data.files![0]!;
        state.shareCalls.push({ name: file.name, type: file.type, size: file.size, activation: navigator.userActivation?.isActive });
        return file.arrayBuffer().then(buffer => {
          state.shareCalls[state.shareCalls.length - 1].bytes = Array.from(new Uint8Array(buffer));
          if (state.shareMode === "cancel") throw new DOMException("Cancelled", "AbortError");
          if (state.shareMode === "error") throw new Error("Share failed");
        });
      } });
    });
    await page.goto(`${fixture.url}/fixture`);
    const url = page.url();
    const prepare = page.getByRole("button", { name: "Last ned Originalt namn.png", exact: true });
    const share = page.getByRole("button", { name: "Lagre eller del", exact: true });
    const status = page.getByRole("status");
    await expect(page.getByRole("link", { name: "Last ned Originalt namn.png" })).toHaveCount(0);
    await prepare.click(); await expect(share).toBeVisible();
    expect(await page.evaluate(() => (window as any).shareCalls)).toEqual([]);
    const usableShareButton = async (button: Locator) => {
      const box = await button.boundingBox();
      expect(box!.x).toBeGreaterThanOrEqual(0); expect(box!.x + box!.width).toBeLessThanOrEqual(390);
      expect(box!.height).toBeGreaterThanOrEqual(44);
      await button.click();
    };
    await page.getByRole("button", { name: "Vis original", exact: true }).click();
    const viewer = page.getByRole("dialog", { name: "Originalt namn.png", exact: true });
    await viewer.getByRole("button", { name: "Last ned Originalt namn.png", exact: true }).click();
    await usableShareButton(viewer.getByRole("button", { name: "Lagre eller del", exact: true }));
    await expect(viewer.getByRole("status")).toContainText("Delinga vart avbroten");
    await page.keyboard.press("Escape"); await expect(viewer).toHaveCount(0);
    await usableShareButton(share); await expect(status).toContainText("Delinga vart avbroten");
    await page.evaluate(() => { (window as any).shareMode = "error"; });
    await share.click(); await expect(status).toContainText("Kunne ikkje opne deling");
    await page.evaluate(() => { (window as any).shareMode = "success"; });
    await share.click(); await expect(share).toHaveCount(0);
    await expect(status).toContainText("Biletet er overlevert til deling.");
    const calls = await page.evaluate(() => (window as any).shareCalls);
    expect(calls).toHaveLength(4);
    for (const call of calls) {
      expect(call).toMatchObject({ name: "Originalt namn.png", type: "image/png", size: 7, bytes: [137, 80, 78, 71, 1, 2, 3] });
      if (call.activation !== undefined) expect(call.activation).toBe(true);
    }
    for (const format of ["heic", "avif"]) {
      responseKind = format;
      await prepare.click(); await expect(share).toBeVisible(); await share.click();
      await expect(status).toContainText("Biletet er overlevert til deling.");
      const latest = await page.evaluate(() => (window as any).shareCalls.at(-1));
      expect(latest).toMatchObject({ name: "Originalt namn.png", type: `image/${format}`, size: 12,
        bytes: [0, 0, 0, 24, ...Array.from(Buffer.from(`ftyp${format}`))] });
    }
    responseKind = "image";
    await page.evaluate(() => { (window as any).canShareFiles = false; });
    await prepare.click(); await expect(status).toContainText("Denne fila kan ikkje delast her"); await expect(share).toHaveCount(0);
    await page.evaluate(() => { (window as any).canShareFiles = true; });
    for (const kind of ["error", "html", "large"]) {
      responseKind = kind; await prepare.click();
      await expect(status).toContainText(kind === "large" ? "for stort" : "Kunne ikkje hente");
      await expect(share).toHaveCount(0);
    }
    await page.evaluate(() => { Object.defineProperty(navigator, "share", { value: undefined }); });
    await prepare.click(); await expect(status).toContainText("støttar ikkje deling av filer");
    await expect(page).toHaveURL(url); expect(context.pages()).toHaveLength(1);
    await expect(page.getByRole("textbox", { name: "Utkast" })).toHaveValue("bevar meg");
    expect(await page.evaluate(() => (window as any).shareCalls.length)).toBe(6);
  } finally { fixture.server.closeAllConnections(); fixture.server.close(); }
});
