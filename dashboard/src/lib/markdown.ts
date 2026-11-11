import DOMPurify from "dompurify";
import { Marked, marked } from "marked";

const escapes: Record<string, string> = { "&": "&amp;", "<": "&lt;", ">": "&gt;" };
/** What a person typed: a line break is a line break, and anything that looks
 * like a tag is text they wrote, shown as written and never dropped. */
const typed = new Marked({
  breaks: true,
  renderer: {
    html: ({ text }) => text.replace(/[&<>]/g, (character) => escapes[character] ?? character),
  },
});

/** A chat title on one line: emphasis and code marks are noise there. */
export function plainTitle(title: string): string {
  return title.replace(/\*\*|`/g, "");
}

/** Archived messages cannot load remote images or run HTML, scripts or styles.
 * `own` renders a message the reader typed themselves. */
export function markdown(text: string, own = false): string {
  const html = own ? typed.parse(text, { async: false }) : marked.parse(text, { async: false });
  return DOMPurify.sanitize(html, {
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
