import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  workers: 1,
  outputDir: "../../mash-artifacts/dashboard-browser",
  use: { baseURL: "http://127.0.0.1:4179", headless: true },
  webServer: {
    command: "pnpm run dev --host 127.0.0.1 --port 4179 --strictPort",
    url: "http://127.0.0.1:4179",
    reuseExistingServer: false,
  },
});
