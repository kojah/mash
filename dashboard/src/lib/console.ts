// The MCP console's data: the server's own tool listing, turned into what a
// form needs, and a tool's answer, summarised for a status line. Nothing here
// knows any particular tool except the grouping below.

/** How a parameter is typed in the form. Anything structured is typed as JSON. */
export type ParamKind = "string" | "integer" | "number" | "boolean" | "enum" | "json";
export interface Param {
  readonly name: string;
  readonly kind: ParamKind;
  /** The schema's own word for the type, shown beside the name. */
  readonly type: string;
  readonly required: boolean;
  readonly about: string;
  readonly choices: readonly string[];
  /** The default the tool uses when the parameter is left out, as JSON. */
  readonly fallback: string;
}
export interface Tool {
  readonly name: string;
  readonly about: string;
  readonly readOnly: boolean;
  readonly group: string;
  /** Its place under that heading: the hand-kept order, then by name. */
  readonly rank: number;
  readonly params: readonly Param[];
}
export interface Console {
  readonly build: string;
  readonly contract: string;
  readonly tools: readonly Tool[];
}

/** The one thing kept by hand: which heading a tool is listed under. A tool
 * missing from here still appears, under "Other". */
const groups: readonly (readonly [string, readonly string[]])[] = [
  [
    "History",
    ["search", "list_chats", "view", "list_attachments", "get_attachment", "query_silver"],
  ],
  [
    "Memory",
    [
      "list_records",
      "get_record",
      "recall_records",
      "find_record_matches",
      "get_record_capture",
      "get_extraction_checkpoint",
    ],
  ],
  ["Service", ["status", "help"]],
];
export const locked = "Changes data";
export const groupOrder: readonly string[] = [...groups.map(([name]) => name), "Other", locked];

type Json = Record<string, unknown>;
function object(value: unknown): Json {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw new Error("Invalid console response. Update the dashboard and retry.");
  return value as Json;
}
const optional = (value: unknown): Json =>
  value && typeof value === "object" && !Array.isArray(value) ? (value as Json) : {};
const string = (value: unknown): string => (typeof value === "string" ? value : "");

/** Follows a `$ref` into the schema's definitions and drops a "or null" wrapper,
 * leaving the schema that describes a value actually given. */
function resolve(schema: Json, definitions: Json): Json {
  const reference = string(schema.$ref).replace(/^#\/(\$defs|definitions)\//, "");
  if (reference)
    return {
      ...resolve(optional(definitions[reference]), definitions),
      ...schema,
      $ref: undefined,
    };
  for (const key of ["anyOf", "oneOf"]) {
    const variants = Array.isArray(schema[key]) ? (schema[key] as unknown[]).map(optional) : [];
    const given = variants.filter((variant) => variant.type !== "null");
    if (variants.length && given.length === 1 && given[0])
      return { ...resolve(given[0], definitions), ...schema, [key]: undefined };
  }
  return schema;
}

function param(name: string, raw: Json, required: boolean, definitions: Json): Param {
  const schema = resolve(raw, definitions);
  const types = (Array.isArray(schema.type) ? schema.type : [schema.type]).filter(
    (type): type is string => typeof type === "string" && type !== "null",
  );
  const choices = Array.isArray(schema.enum)
    ? schema.enum.filter((choice): choice is string => typeof choice === "string")
    : [];
  const type = choices.length ? "enum" : (types[0] ?? "json");
  const kind: ParamKind = choices.length
    ? "enum"
    : type === "string" || type === "integer" || type === "number" || type === "boolean"
      ? type
      : "json";
  const fallback = raw.default ?? schema.default;
  return {
    name,
    kind,
    type,
    required,
    about: string(raw.description) || string(schema.description),
    choices,
    fallback: fallback === undefined || fallback === null ? "" : JSON.stringify(fallback),
  };
}

export function parseConsole(value: unknown): Console {
  const root = object(value);
  if (!Array.isArray(root.tools)) throw new Error("Invalid console tool list.");
  const tools = root.tools.map((entry): Tool => {
    const tool = object(entry);
    const name = string(tool.name);
    if (!name) throw new Error("Invalid console tool.");
    const schema = optional(tool.inputSchema);
    const definitions = { ...optional(schema.definitions), ...optional(schema.$defs) };
    const required = Array.isArray(schema.required) ? schema.required : [];
    const readOnly = optional(tool.annotations).readOnlyHint === true;
    const params = Object.entries(optional(schema.properties))
      .map(([key, raw]) => param(key, optional(raw), required.includes(key), definitions))
      // Required parameters first; the rest keep the server's order.
      .sort((a, b) => Number(b.required) - Number(a.required));
    const listed = readOnly ? groups.find(([, names]) => names.includes(name)) : undefined;
    return {
      name,
      about: string(tool.description),
      readOnly,
      group: readOnly ? (listed?.[0] ?? "Other") : locked,
      rank: listed ? listed[1].indexOf(name) : Number.MAX_SAFE_INTEGER,
      params,
    };
  });
  tools.sort((a, b) => a.rank - b.rank || a.name.localeCompare(b.name));
  return { build: string(root.build_revision), contract: string(root.contract_fingerprint), tools };
}

/** The first sentence of a tool's description, for its row in the list. */
export function headline(about: string): string {
  return /^.*?[.!?](?=\s|$)/.exec(about)?.[0] ?? about;
}

export type Arguments =
  | { readonly ok: true; readonly arguments: Record<string, unknown> }
  | { readonly ok: false; readonly param: string; readonly message: string };

/** The arguments a filled form stands for. Blank fields are left out, so the
 * tool applies its own defaults. */
export function argumentsFor(tool: Tool, values: Readonly<Record<string, string>>): Arguments {
  const result: Record<string, unknown> = {};
  for (const entry of tool.params) {
    const typed = (values[`${tool.name}.${entry.name}`] ?? "").trim();
    if (!typed) {
      if (entry.required)
        return { ok: false, param: entry.name, message: `${entry.name} is required.` };
      continue;
    }
    if (entry.kind === "integer") {
      if (!/^-?\d+$/.test(typed) || !Number.isSafeInteger(Number(typed)))
        return { ok: false, param: entry.name, message: `${entry.name} must be a whole number.` };
      result[entry.name] = Number(typed);
    } else if (entry.kind === "number") {
      if (!Number.isFinite(Number(typed)))
        return { ok: false, param: entry.name, message: `${entry.name} must be a number.` };
      result[entry.name] = Number(typed);
    } else if (entry.kind === "boolean") result[entry.name] = typed === "true";
    else if (entry.kind === "json") {
      try {
        result[entry.name] = JSON.parse(typed);
      } catch {
        return {
          ok: false,
          param: entry.name,
          message: `${entry.name} must be JSON, such as ["a"] or {"key": "value"}.`,
        };
      }
    } else result[entry.name] = typed;
  }
  return { ok: true, arguments: result };
}

export interface Answer {
  readonly failed: boolean;
  readonly reply: unknown;
  readonly text: string;
}
export function parseAnswer(value: unknown): Answer {
  const root = object(value);
  if (typeof root.failed !== "boolean") throw new Error("Invalid console answer.");
  return { failed: root.failed, reply: root.reply ?? null, text: string(root.text) };
}

export interface Summary {
  /** Busy is a failure the reply itself says is worth trying again. */
  readonly state: "ok" | "failed" | "busy";
  readonly heading: string;
  /** How many entries the reply's first list holds, when it has one. */
  readonly items: number | null;
}
export function summarize(answer: Answer): Summary {
  const reply = optional(answer.reply);
  if (answer.failed) {
    const error = optional(reply.error);
    return {
      state: error.retryable === true ? "busy" : "failed",
      heading:
        string(error.message) || answer.text.split("\n")[0] || "The tool reported a failure.",
      items: null,
    };
  }
  const first = Array.isArray(answer.reply)
    ? answer.reply
    : Object.values(reply).find((value) => Array.isArray(value));
  return { state: "ok", heading: "", items: Array.isArray(first) ? first.length : null };
}
