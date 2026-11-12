// The reader's choice of light or dark. Dark is the default; a choice is kept
// in this browser. The page's colours follow the attribute on the root element.
export type Theme = "dark" | "light";
const KEY = "mash-theme";

function stored(): Theme {
  try {
    return localStorage.getItem(KEY) === "light" ? "light" : "dark";
  } catch {
    return "dark"; // Storage can be unavailable, as in a private window.
  }
}

export const theme = $state<{ current: Theme }>({ current: "dark" });

function apply(next: Theme): void {
  theme.current = next;
  if (next === "light") document.documentElement.dataset.theme = "light";
  else delete document.documentElement.dataset.theme;
}

/** Applies the kept choice before the page is drawn. */
export function restoreTheme(): void {
  apply(stored());
}

export function toggleTheme(): void {
  const next: Theme = theme.current === "dark" ? "light" : "dark";
  apply(next);
  try {
    localStorage.setItem(KEY, next);
  } catch {
    // The choice still holds for this visit.
  }
}
