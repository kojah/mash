import { mount } from "svelte";
import "./app.css";
import "./terminal.css";

import App from "./App.svelte";
import { installNavigationBoundary } from "./lib/navigation";

const target = document.getElementById("app");
if (target === null) throw new Error("missing #app element");

const removeNavigationBoundary = installNavigationBoundary(document);
import.meta.hot?.dispose(removeNavigationBoundary);

export default mount(App, { target });
