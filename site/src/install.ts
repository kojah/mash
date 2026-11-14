/**
 * The landing hero's install picker: choose an operating system and an install
 * method, and the command and its prompt follow.
 */

interface Method {
  readonly id: string;
  readonly label: string;
  readonly prompt: string;
  readonly command: string;
}

interface System {
  readonly id: string;
  readonly label: string;
  readonly methods: readonly Method[];
}

const STORED = "mash-install";

function isMethod(value: unknown): value is Method {
  if (typeof value !== "object" || value === null) return false;
  const method = value as Record<string, unknown>;
  return ["id", "label", "prompt", "command"].every((key) => typeof method[key] === "string");
}

function isSystem(value: unknown): value is System {
  if (typeof value !== "object" || value === null) return false;
  const { id, label, methods } = value as Record<string, unknown>;
  return (
    typeof id === "string" &&
    typeof label === "string" &&
    Array.isArray(methods) &&
    methods.length > 0 &&
    methods.every(isMethod)
  );
}

function parseSystems(text: string | undefined): System[] {
  try {
    const parsed: unknown = JSON.parse(text ?? "[]");
    return Array.isArray(parsed) ? parsed.filter(isSystem) : [];
  } catch {
    return [];
  }
}

/** The visitor's operating system, as far as the browser says. */
export function detectSystem(platform: string): string {
  const name = platform.toLowerCase();
  if (name.includes("win")) return "windows";
  if (name.includes("mac") || name.includes("iphone") || name.includes("ipad")) return "macos";
  return "linux";
}

function remembered(): { system?: string; method?: string } {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(STORED) ?? "{}");
    if (typeof parsed !== "object" || parsed === null) return {};
    const { system, method } = parsed as Record<string, unknown>;
    return {
      ...(typeof system === "string" ? { system } : {}),
      ...(typeof method === "string" ? { method } : {}),
    };
  } catch {
    return {};
  }
}

function remember(system: string, method: string): void {
  try {
    localStorage.setItem(STORED, JSON.stringify({ system, method }));
  } catch {
    // The choice simply is not kept.
  }
}

/** Wires one picker. It opens on the install script for the visitor's own system. */
export function mountInstall(picker: HTMLElement): void {
  const systems = parseSystems(picker.dataset.systems);
  const systemRow = picker.querySelector<HTMLElement>('[data-pick="system"]');
  const methodRow = picker.querySelector<HTMLElement>('[data-pick="method"]');
  const prompt = picker.querySelector<HTMLElement>(".picked .prompt");
  const code = picker.querySelector<HTMLElement>(".picked code");
  const copy = picker.querySelector<HTMLButtonElement>(".picked button[data-copy]");
  if (picker.dataset.mounted || !systemRow || !methodRow || !prompt || !code || !copy) {
    return;
  }
  const [fallback] = systems;
  if (!fallback) return;
  picker.dataset.mounted = "true";
  const saved = remembered();

  function show(systemId: string, methodId: string | undefined, keep: boolean): void {
    const system = systems.find((candidate) => candidate.id === systemId) ?? fallback;
    if (!system) return;
    const [preferred] = system.methods;
    const method = system.methods.find((candidate) => candidate.id === methodId) ?? preferred;
    if (!method) return;
    for (const button of systemRow?.querySelectorAll("button") ?? []) {
      button.setAttribute("aria-pressed", String(button.value === system.id));
    }
    methodRow?.replaceChildren(
      ...system.methods.map((candidate) => {
        const button = document.createElement("button");
        button.type = "button";
        button.value = candidate.id;
        const marked = (key: string) =>
          picker.querySelector<HTMLTemplateElement>(
            `template[data-method-icon="${CSS.escape(key)}"]`,
          );
        const icon = marked(`${system.id}:${candidate.id}`) ?? marked(candidate.id);
        if (icon) button.append(icon.content.cloneNode(true));
        button.append(candidate.label);
        button.setAttribute("aria-pressed", String(candidate.id === method.id));
        return button;
      }),
    );
    if (prompt) prompt.textContent = method.prompt;
    if (code) code.textContent = method.command;
    if (copy) copy.dataset.copy = method.command;
    picker.dataset.system = system.id;
    picker.dataset.method = method.id;
    if (keep) remember(system.id, method.id);
  }

  picker.addEventListener("click", (event) => {
    const button =
      event.target instanceof Element
        ? event.target.closest<HTMLButtonElement>("[data-pick] button")
        : null;
    if (!button) return;
    const row = button.closest<HTMLElement>("[data-pick]")?.dataset.pick;
    if (row === "system") show(button.value, picker.dataset.method, true);
    else show(picker.dataset.system ?? fallback.id, button.value, true);
  });

  show(
    saved.system ?? detectSystem(navigator.platform || navigator.userAgent),
    saved.method,
    false,
  );
}
