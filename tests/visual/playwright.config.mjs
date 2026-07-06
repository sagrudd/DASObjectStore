import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: ".",
  outputDir: "../../test-results/visual-workspaces",
  reporter: [["list"], ["html", { open: "never" }]],
  use: {
    browserName: "chromium",
    colorScheme: "light",
    viewport: devices["Desktop Chrome"].viewport
  }
});
