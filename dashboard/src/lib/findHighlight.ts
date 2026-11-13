// Paints Find in chat matches on the rendered page with the CSS Custom
// Highlight API, which marks text without changing the page's markup.
// Browsers without it still scroll to each match; they only lose the colour.

const ALL = "mash-find";
const CURRENT = "mash-find-current";

function supported(): boolean {
  return typeof Highlight !== "undefined" && typeof CSS !== "undefined" && "highlights" in CSS;
}

function pattern(query: string): RegExp | undefined {
  const words = query.trim().split(/\s+/).filter(Boolean);
  if (!words.length) return undefined;
  const escaped = words.map((word) => word.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"));
  return new RegExp(escaped.join("\\s+"), "giu");
}

function ranges(root: Node, expression: RegExp): Range[] {
  const found: Range[] = [];
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    const text = node.nodeValue ?? "";
    expression.lastIndex = 0;
    for (let match = expression.exec(text); match; match = expression.exec(text)) {
      if (!match[0]) break;
      const range = document.createRange();
      range.setStart(node, match.index);
      range.setEnd(node, match.index + match[0].length);
      found.push(range);
    }
  }
  return found;
}

export function clearMatches(): void {
  if (!supported()) return;
  CSS.highlights.delete(ALL);
  CSS.highlights.delete(CURRENT);
}

/**
 * Highlights every match under `root`, and the `occurrence`-th one inside
 * `current` more strongly. Returns that match, or `current` itself when
 * its text is not on the page (for example, cut from a long tool output).
 */
export function paintMatches(
  root: HTMLElement,
  query: string,
  current?: { readonly element: Element; readonly occurrence: number },
): Range | Element | undefined {
  const expression = pattern(query);
  if (!expression) {
    clearMatches();
    return undefined;
  }
  const all = ranges(root, expression);
  let target: Range | undefined;
  if (current) {
    const inside = all.filter((range) => current.element.contains(range.startContainer));
    target = inside[Math.min(current.occurrence, inside.length - 1)];
  }
  if (supported()) {
    CSS.highlights.set(ALL, new Highlight(...all.filter((range) => range !== target)));
    if (target) CSS.highlights.set(CURRENT, new Highlight(target));
    else CSS.highlights.delete(CURRENT);
  }
  return target ?? current?.element;
}

/**
 * Scrolls `target` to the middle of the window, first scrolling any box it
 * sits in that scrolls on its own (a long tool output, a wide command).
 */
export function bringIntoView(target: Range | Element): void {
  const start = target instanceof Range ? target.startContainer.parentElement : target;
  for (let box = start; box && box !== document.body; box = box.parentElement) {
    const style = getComputedStyle(box);
    const scrolls =
      /(auto|scroll)/.test(`${style.overflowX} ${style.overflowY}`) &&
      (box.scrollWidth > box.clientWidth || box.scrollHeight > box.clientHeight);
    if (!scrolls) continue;
    const spot = target.getBoundingClientRect();
    const frame = box.getBoundingClientRect();
    box.scrollLeft += spot.left - frame.left - box.clientWidth / 2;
    box.scrollTop += spot.top - frame.top - box.clientHeight / 2;
  }
  const spot = target.getBoundingClientRect();
  window.scrollTo({ top: window.scrollY + spot.top - window.innerHeight / 2, behavior: "instant" });
}
