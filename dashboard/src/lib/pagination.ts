export interface ListPage<T> {
  readonly items: readonly T[];
  readonly page: number;
  readonly pageCount: number;
  /** One-based positions of the first and last items shown; 0 when empty. */
  readonly first: number;
  readonly last: number;
  readonly total: number;
}

/** One page of a list, moving a page past the end back to the last page. */
export function pageOf<T>(items: readonly T[], requested: number, size: number): ListPage<T> {
  const total = items.length;
  const pageCount = Math.max(1, Math.ceil(total / size));
  const page = Math.min(Math.max(1, requested), pageCount);
  const start = (page - 1) * size;
  const shown = items.slice(start, start + size);
  return {
    items: shown,
    page,
    pageCount,
    first: total === 0 ? 0 : start + 1,
    last: start + shown.length,
    total,
  };
}
