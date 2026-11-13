import starlight from "@astrojs/starlight";
import { defineConfig } from "astro/config";

export default defineConfig({
  site: "https://getmash.io",
  integrations: [
    starlight({
      title: "mash",
      favicon: "/favicon.svg",
      description: "The Multi-Agent Search History Tool",
      // The header is the project logo drawn live: the mark, the name and its
      // cursor, in the dark colouring or the light one to suit the theme.
      logo: { dark: "./public/mash-mark.svg", light: "./public/mash-mark-light.svg" },
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
        Footer: "./src/components/Footer.astro",
        Head: "./src/components/Head.astro",
        Hero: "./src/components/Hero.astro",
        Search: "./src/components/Search.astro",
        SocialIcons: "./src/components/SocialIcons.astro",
        ThemeSelect: "./src/components/ThemeSelect.astro",
      },
      sidebar: [
        {
          label: "Start here",
          items: [
            { label: "Welcome to Mash", slug: "index" },
            { label: "Quick start", slug: "quick-start" },
            { label: "Installation", slug: "installation" },
            "search",
            "agents",
            "dashboard",
            "cli",
            "configuration",
            "mcp",
            "tools",
          ],
        },
        {
          label: "Using Mash",
          items: [
            "records",
            "services",
            "synchronization",
            "indexing",
            "shared-index",
            "upgrades",
            "troubleshooting",
            "privacy",
          ],
        },
      ],
    }),
  ],
});
