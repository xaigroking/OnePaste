import { defineConfig } from "@playwright/test";

// Screenshot and keyboard tests of the front end alone: the page is built with a stand-in for the api module
// (vite --mode mock), so no desktop program is needed. The baselines are in tests/screens/.
export default defineConfig({
  testDir: "tests",
  snapshotPathTemplate: "{testDir}/screens/{arg}{ext}",
  fullyParallel: true,
  reporter: "list",
  expect: { toHaveScreenshot: { maxDiffPixelRatio: 0.01, animations: "disabled" } },
  use: { baseURL: "http://127.0.0.1:4173", colorScheme: "light", locale: "en-US" },
  webServer: {
    command: "npx vite build --mode mock && npx vite preview --mode mock --outDir dist-mock --host 127.0.0.1 --port 4173 --strictPort",
    url: "http://127.0.0.1:4173",
    reuseExistingServer: false,
    timeout: 120_000,
  },
});
