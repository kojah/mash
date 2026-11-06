import starlight from "@astrojs/starlight";
import { defineConfig } from "astro/config";

export default defineConfig({
  site: "https://mashapp.dev",
  integrations: [
    starlight({
      title: "mash",
      favicon: "/mash-logo.png",
      description: "The Multi-Agent Search History Tool",
      logo: { src: "./public/mash-logo.png" },
      social: [{ icon: "github", label: "GitHub", href: "https://github.com/kojah/mash" }],
      customCss: ["./src/styles/custom.css"],
      components: {
        Head: "./src/components/Head.astro",
        Search: "./src/components/Search.astro",
      },
      sidebar: [
        {
          label: "Start here",
          items: [
            { label: "Welcome to Mash", slug: "index" },
            { label: "Quick start", slug: "quick-start" },
            { label: "Installation", slug: "installation" },
          ],
        },
        {
          label: "Using Mash",
          items: [
            "search",
            "agents",
            "dashboard",
            "synchronization",
            "indexing",
            "shared-index",
            "upgrades",
            "troubleshooting",
            "privacy",
          ],
        },
        {
          label: "More help",
          collapsed: true,
          items: ["cli", "configuration", "mcp"],
        },
      ],
    }),
  ],
});
