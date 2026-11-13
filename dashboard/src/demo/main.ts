// The demo's entry point: the same dashboard, answered from made-up history
// in the browser instead of by `mash dash`. Nothing outside this folder knows
// the demo exists.
import { mount } from "svelte";
import { install, installEvents } from "../lib/transport";
import Banner from "./Banner.svelte";
import { transport } from "./backend";

install(transport);
// Made-up history never changes, so there is nothing to listen for.
installEvents(undefined);
const bar = document.createElement("div");
document.body.append(bar);
mount(Banner, { target: bar });

await import("../main");
