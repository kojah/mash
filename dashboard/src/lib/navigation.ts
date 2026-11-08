/** Keep app links and forms inside the current document, even if a component
 * forgets preventDefault. Browser controls and external links remain native. */
export function installNavigationBoundary(document: Document): () => void {
  const view = document.defaultView;
  if (!view) throw new Error("Navigation requires a browser window");
  const click = (event: MouseEvent) => {
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey)
      return;
    const target = event.target;
    if (!(target instanceof Element)) return;
    const anchor = target.closest("a[href]");
    if (!(anchor instanceof HTMLAnchorElement) || anchor.hasAttribute("download")) return;
    if (anchor.target && anchor.target !== "_self") return;
    const destination = new URL(anchor.href);
    const current = view.location;
    if (
      destination.origin !== current.origin ||
      destination.pathname !== current.pathname ||
      destination.search !== current.search
    )
      return;
    event.preventDefault();
    // Setting a fragment cannot request a new document. Bare home links are
    // normalized too, instead of letting the browser reload the current URL.
    const hash = destination.hash || "#/";
    if (current.hash !== hash) current.hash = hash;
  };
  const submit = (event: SubmitEvent) => event.preventDefault();
  document.addEventListener("click", click, true);
  document.addEventListener("submit", submit, true);
  return () => {
    document.removeEventListener("click", click, true);
    document.removeEventListener("submit", submit, true);
  };
}
