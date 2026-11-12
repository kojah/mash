import process from "node:process";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vitest/config";

// `mash dash` serves the built files; during development the dev server
// forwards API calls to a running `mash dash`.
//
// `--mode demo` builds the same dashboard for getmash.io/demo, where it is
// answered from made-up history in the browser (see src/demo).
export default defineConfig(({ mode }) => {
  const demo = mode === "demo";
  return {
    base: demo ? "/demo/" : "/",
    plugins: [
      tailwindcss(),
      svelte(),
      ...(demo
        ? [
            {
              name: "mash-demo-entry",
              transformIndexHtml: {
                order: "pre" as const,
                handler: (html: string) => html.replace("/src/main.ts", "/src/demo/main.ts"),
              },
            },
          ]
        : []),
    ],
    build: { outDir: demo ? "dist-demo" : "dist", emptyOutDir: true, sourcemap: false },
    server: {
      proxy: demo
        ? {}
        : {
            "/api": {
              target: process.env.MASH_DASHBOARD_API_URL || "http://127.0.0.1:8790",
              changeOrigin: true,
            },
          },
    },
    test: { environment: "node", include: ["src/**/*.test.ts"] },
  };
});
