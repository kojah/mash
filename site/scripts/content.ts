import { copyFile, mkdir, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { renderMarkdown } from "./markdown.ts";

const repo = new URL("../../", import.meta.url);
const output = new URL("../src/content/docs/", import.meta.url);
const names = (await readdir(new URL("docs/", repo))).filter((name) => name.endsWith(".md"));
const routes = new Map(
  names.map((name) => [
    `docs/${name}`,
    name === "README.md" ? "/guides/" : `/${name.slice(0, -3)}/`,
  ]),
);
routes.set("README.md", "/quick-start/");
routes.set("docs/home.mdx", "/");
await rm(output, { recursive: true, force: true });
await mkdir(output, { recursive: true });
await mkdir(new URL("../public/", import.meta.url), { recursive: true });
await copyFile(
  new URL("assets/mash-logo.png", repo),
  new URL("../public/mash-logo.png", import.meta.url),
);
for (const source of [...names.map((name) => `docs/${name}`), "README.md"]) {
  let body = await readFile(new URL(source, repo), "utf8");
  const title = source === "README.md" ? "Quick start" : body.match(/^# (.+)$/m)?.[1];
  if (!title) throw new Error(`Missing title in ${source}`);
  if (source === "README.md") {
    const start = body.indexOf("## Quick start");
    if (start < 0) throw new Error("README is missing its Quick start section");
    body = body.slice(start).replace("## Quick start", '<span id="quick-start"></span>');
  } else body = body.replace(/^# .+\n/m, "");
  const route = routes.get(source);
  if (!route) throw new Error(`Missing route for ${source}`);
  const filename = route.slice(1, -1);
  const frontmatter = `---\ntitle: ${JSON.stringify(title)}\neditUrl: ${JSON.stringify(`https://github.com/kojah/mash/edit/main/${source}`)}\n---\n`;
  await writeFile(
    new URL(`${filename}.md`, output),
    frontmatter + renderMarkdown(body, source, routes),
  );
}
await copyFile(new URL("docs/home.mdx", repo), new URL("index.mdx", output));
console.log(`Prepared documentation in ${fileURLToPath(output)}`);
