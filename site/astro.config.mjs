import starlight from "@astrojs/starlight";
import { defineConfig } from "astro/config";

export default defineConfig({
  site: "https://mashapp.dev",
  integrations: [
    starlight({
      title: "mash",
      favicon: "/favicon.svg",
      description: "The Multi-Agent Search History Tool",
      // On the cream of the light theme the mark sits on its own dark tile.
      logo: { dark: "./public/mash-mark.svg", light: "./public/favicon.svg" },
      social: [{ icon: "github", label: "GitHub", href: "https://github.com/kojah/mash" }],
      // The fonts are bundled with the site, as they are with the dashboard.
      customCss: [
        "@fontsource/ibm-plex-sans/latin-400.css",
        "@fontsource/ibm-plex-sans/latin-400-italic.css",
        "@fontsource/ibm-plex-sans/latin-500.css",
        "@fontsource/ibm-plex-sans/latin-600.css",
        "@fontsource/ibm-plex-mono/latin-400.css",
        "@fontsource/ibm-plex-mono/latin-500.css",
        "./src/styles/custom.css",
      ],
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
            "services",
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
