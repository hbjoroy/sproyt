import type { Page } from "@playwright/test";
export async function eventImage(page: Page) {
  const data = await page.evaluate(() => {
    const canvas = document.createElement("canvas"); canvas.width = 640; canvas.height = 320;
    const context = canvas.getContext("2d")!;
    context.fillStyle = "#243a36"; context.fillRect(0, 0, 640, 320);
    context.fillStyle = "#dcea7d"; context.font = "bold 60px sans-serif"; context.fillText("KONSERT", 48, 150);
    context.font = "26px sans-serif"; context.fillText("Grendahuset · 2027", 48, 210);
    return canvas.toDataURL("image/png").split(",")[1]!;
  });
  return Buffer.from(data, "base64");
}
