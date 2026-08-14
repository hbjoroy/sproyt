import { defineConfig, devices } from "@playwright/test";

const port = Number(process.env.SPROYT_E2E_PORT);
if (!Number.isInteger(port) || port < 1024 || port > 65535) {
  throw new Error("SPROYT_E2E_PORT must be a reserved local TCP port");
}
const baseURL = `http://127.0.0.1:${port}`;

export default defineConfig({
  testDir: "./tests",
  globalSetup: "./tests/global-setup.ts",
  fullyParallel: false,
  workers: 1,
  forbidOnly: Boolean(process.env.CI),
  retries: 0,
  failOnFlakyTests: true,
  reporter: process.env.CI ? [["github"], ["html", { open: "never" }]] : "list",
  use: { baseURL, serviceWorkers: "block", trace: "retain-on-failure" },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }]
});
