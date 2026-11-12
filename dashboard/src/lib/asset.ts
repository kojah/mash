/** The address of a file in `public/`, wherever the dashboard is served from. */
export function asset(path: string): string {
  return `${import.meta.env.BASE_URL}${path}`;
}
