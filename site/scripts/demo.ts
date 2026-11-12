// Places the dashboard's demo build inside the built site, at /demo/.
// Build it first with `pnpm build:demo` in ../dashboard.
import { cp, stat } from "node:fs/promises";

const built = new URL("../../dashboard/dist-demo/", import.meta.url);
const found = await stat(new URL("index.html", built)).catch(() => null);
if (!found) throw new Error("No demo build. Run `pnpm build:demo` in dashboard/ first.");
await cp(built, new URL("../dist/demo/", import.meta.url), { recursive: true });
console.log("Added the dashboard demo at /demo/");
