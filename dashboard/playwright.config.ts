import process from "node:process";
import { defineConfig } from "@playwright/test";

const liveUrl = process.env.MASH_DASHBOARD_URL;

export default defineConfig({
  testDir: "./e2e",
  testMatch: liveUrl ? "**/*.live.ts" : "**/*.spec.ts",
  workers: 1,
  outputDir: "../../mash-artifacts/dashboard-browser",
  use: {
    baseURL: liveUrl || "http://127.0.0.1:4179",
    headless: true,
    ...(process.env.MASH_BROWSER_EXECUTABLE
      ? { launchOptions: { executablePath: process.env.MASH_BROWSER_EXECUTABLE } }
      : {}),
  },
  ...(liveUrl
    ? {}
    : {
        webServer: {
          command: "pnpm run dev --host 127.0.0.1 --port 4179 --strictPort",
          url: "http://127.0.0.1:4179",
          reuseExistingServer: false,
        },
      }),
});
