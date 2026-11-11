import { mount } from "svelte";
// The fonts ship with the dashboard: it is served on a private network and
// never fetches type from a font service.
import "@fontsource/ibm-plex-sans/latin-400.css";
import "@fontsource/ibm-plex-sans/latin-400-italic.css";
import "@fontsource/ibm-plex-sans/latin-500.css";
import "@fontsource/ibm-plex-sans/latin-600.css";
import "@fontsource/ibm-plex-mono/latin-400.css";
import "@fontsource/ibm-plex-mono/latin-500.css";
import "./app.css";
import "./terminal.css";

import App from "./App.svelte";
import { installNavigationBoundary } from "./lib/navigation";

const target = document.getElementById("app");
if (target === null) throw new Error("missing #app element");

const removeNavigationBoundary = installNavigationBoundary(document);
import.meta.hot?.dispose(removeNavigationBoundary);

export default mount(App, { target });
