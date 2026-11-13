// Find in chat: every place a phrase occurs in a whole chat, in reading
// order. Matching ignores case and treats runs of whitespace as one space,
// so a phrase still matches across a line break.

export interface FindSource {
  readonly id: string;
  readonly text: string;
}

export interface FindMatch {
  /** The message holding this occurrence. */
  readonly messageId: string;
  /** The message's position in the chat, counting from zero. */
  readonly position: number;
  /** Which occurrence within that message, counting from zero. */
  readonly occurrence: number;
}

/** More matches than anyone steps through; past this the count says "+". */
export const MATCH_LIMIT = 5000;

export function normalize(text: string): string {
  return text.toLocaleLowerCase().replace(/\s+/g, " ");
}

export function findMatches(
  messages: readonly FindSource[],
  query: string,
  limit = MATCH_LIMIT,
): FindMatch[] {
  const needle = normalize(query.trim());
  if (!needle) return [];
  const matches: FindMatch[] = [];
  for (const [position, message] of messages.entries()) {
    const text = normalize(message.text);
    let occurrence = 0;
    for (let at = text.indexOf(needle); at !== -1; at = text.indexOf(needle, at + needle.length)) {
      matches.push({ messageId: message.id, position, occurrence });
      occurrence += 1;
      if (matches.length >= limit) return matches;
    }
  }
  return matches;
}

/** "3 of 17", or "3 of 5,000+" when the search stopped at the limit. */
export function countLabel(index: number, total: number, limit = MATCH_LIMIT): string {
  if (!total) return "No matches";
  const of = total >= limit ? `${limit.toLocaleString()}+` : total.toLocaleString();
  return `${(index + 1).toLocaleString()} of ${of}`;
}
