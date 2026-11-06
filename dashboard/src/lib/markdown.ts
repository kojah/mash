import DOMPurify from "dompurify";
import { marked } from "marked";

/** Archived messages cannot load remote images or run HTML, scripts or styles. */
export function markdown(text: string): string {
  return DOMPurify.sanitize(marked.parse(text, { async: false }), {
    ALLOWED_TAGS: [
      "p",
      "br",
      "strong",
      "em",
      "del",
      "blockquote",
      "ul",
      "ol",
      "li",
      "pre",
      "code",
      "h1",
      "h2",
      "h3",
      "h4",
      "hr",
      "a",
      "table",
      "thead",
      "tbody",
      "tr",
      "th",
      "td",
    ],
    ALLOWED_ATTR: ["href", "title"],
  });
}
