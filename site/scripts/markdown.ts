import path from "node:path";
import remarkGfm from "remark-gfm";
import remarkParse from "remark-parse";
import remarkStringify from "remark-stringify";
import { unified } from "unified";
import { visit } from "unist-util-visit";

/** Resolve repository Markdown links without changing examples inside code blocks. */
export function renderMarkdown(body: string, source: string, routes: ReadonlyMap<string, string>) {
  const processor = unified().use(remarkParse).use(remarkGfm).use(remarkStringify);
  const tree = processor.parse(body);
  visit(tree, (node) => {
    if (node.type !== "link" && node.type !== "image" && node.type !== "definition") return;
    if (/^(?:[a-z][a-z\d+.-]*:|\/|#)/i.test(node.url)) return;
    const target = new URL(node.url, `https://github.com/mashium/mash/blob/main/${source}`);
    const file = path.posix.normalize(
      path.posix.join(path.posix.dirname(source), node.url.split(/[?#]/)[0] ?? ""),
    );
    const route = routes.get(file);
    node.url = route ? `${route}${target.search}${target.hash}` : target.href;
  });
  return processor.stringify(tree);
}
