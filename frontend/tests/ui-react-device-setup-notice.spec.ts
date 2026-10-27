import { expect, test, type Page } from "@playwright/test";
import { build } from "esbuild";
import { fileURLToPath } from "node:url";

async function openNotice(page: Page, notification: "needs-setup" | "blocked" | "none", install: "native" | "ios" | "none", installError?: string) {
  const bundle = await build({
    stdin: {
      contents: `import { createRoot } from "react-dom/client";
        import { useState } from "react";
        import baseCss from "@sproyt/ui/styles.css";
        import { DeviceSetupNotice } from "./src/ui/react/device-setup-notice";
        function Example() {
          const [dismissed, setDismissed] = useState({ notifications: false, install: false });
          return <div className="sp-theme" data-density="compact" data-theme="dark"><style>{baseCss}</style>
            <DeviceSetupNotice notification=${JSON.stringify(notification)} install=${JSON.stringify(install)} installError={${JSON.stringify(installError)}}
              dismissedNotifications={dismissed.notifications} dismissedInstall={dismissed.install}
              onDismiss={kind => setDismissed(value => ({ ...value, [kind]: true }))}
              onOpenNotifications={() => { document.body.dataset.notifications = "opened"; }}
              onInstall={() => { document.body.dataset.install = "opened"; }} />
          </div>;
        }
        createRoot(document.getElementById("app")).render(<Example />);`,
      resolveDir: fileURLToPath(new URL("..", import.meta.url)), loader: "tsx"
    }, bundle: true, write: false, jsx: "automatic", format: "iife", loader: { ".css": "text" }
  });
  await page.setContent('<div id="app"></div>');
  await page.addScriptTag({ content: bundle.outputFiles[0]!.text });
}

test("device notices present one optional action at a time and dismiss independently", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 700 });
  await openNotice(page, "needs-setup", "native");
  const notice = page.getByRole("region", { name: "Påminning om varsel" });
  await expect(notice).toBeVisible();
  await expect(page.getByRole("region", { name: "Påminning om Sprøyt som app" })).toHaveCount(0);
  await expect(page.locator("body")).not.toHaveAttribute("data-notifications");
  await notice.getByRole("button", { name: "Opne varslingsinnstillingar" }).click();
  await expect(page.locator("body")).toHaveAttribute("data-notifications", "opened");
  const dismiss = notice.getByRole("button", { name: "Lukk påminning om varsel" });
  const size = await dismiss.boundingBox();
  expect(size!.width).toBeGreaterThanOrEqual(44);
  expect(size!.height).toBeGreaterThanOrEqual(44);
  expect(await notice.evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
  await dismiss.click();
  const install = page.getByRole("region", { name: "Påminning om Sprøyt som app" });
  await expect(install).toBeVisible();
  await expect(page.locator("body")).not.toHaveAttribute("data-install");
  await install.getByRole("button", { name: "Installer appen" }).click();
  await expect(page.locator("body")).toHaveAttribute("data-install", "opened");
  await install.getByRole("button", { name: "Lukk påminning om Sprøyt som app" }).click();
  await expect(install).toHaveCount(0);
});

test("iOS installation guidance precedes blocked notifications and can be closed", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 700 });
  await openNotice(page, "blocked", "ios");
  const install = page.getByRole("region", { name: "Påminning om Sprøyt som app" });
  const how = install.getByRole("button", { name: "Slik gjer du" });
  await expect(how).toHaveAttribute("aria-expanded", "false");
  await how.click();
  await expect(how).toHaveAttribute("aria-expanded", "true");
  await expect(install.getByText(/delingsmenyen/)).toBeVisible();
  await install.getByRole("button", { name: "Lukk påminning om Sprøyt som app" }).click();
  const notice = page.getByRole("region", { name: "Påminning om varsel" });
  await expect(notice).toContainText("Varsel er blokkerte i nettlesaren");
  await notice.getByRole("button", { name: "Opne varslingsinnstillingar" }).click();
  await expect(page.locator("body")).toHaveAttribute("data-notifications", "opened");
});

test("an installation error remains visible when the browser no longer offers a prompt", async ({ page }) => {
  await openNotice(page, "needs-setup", "none", "Prøv å installere frå nettlesarmenyen.");
  const notice = page.getByRole("region", { name: "Påminning om Sprøyt som app" });
  await expect(notice.getByRole("alert")).toHaveText("Prøv å installere frå nettlesarmenyen.");
  await expect(notice.getByRole("button", { name: "Installer appen" })).toHaveCount(0);
  await notice.getByRole("button", { name: "Lukk påminning om Sprøyt som app" }).click();
  await expect(page.getByRole("region", { name: "Påminning om varsel" })).toBeVisible();
});
