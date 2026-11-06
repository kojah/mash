import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests",
  workers: 1,
  use: { baseURL: "http://127.0.0.1:8793" },
  webServer: {
    command: "pnpm exec astro preview --ignore-lock --host 127.0.0.1 --port 8793",
    url: "http://127.0.0.1:8793",
    reuseExistingServer: false,
  },
});
