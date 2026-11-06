import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vitest/config";

// `mash dash` serves the built files; during development the dev server
// forwards API calls to a running `mash dash`.
export default defineConfig({
  plugins: [tailwindcss(), svelte()],
  build: { outDir: "dist", emptyOutDir: true, sourcemap: false },
  server: { proxy: { "/api": { target: "http://127.0.0.1:8790", changeOrigin: true } } },
  test: { environment: "node", include: ["src/**/*.test.ts"] },
});
