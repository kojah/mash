// Whether this browser may read history. A dashboard on the machine itself
// asks for nothing; one reached from other devices asks for its access token
// once and then keeps a cookie the page cannot read.
import { send } from "./transport";

/** `checking` until the dashboard has said whether it wants a sign-in. */
export type Gate = "checking" | "open" | "locked";

export const gate = $state<{ value: Gate }>({ value: "checking" });

/** What `/api/session` answers, or undefined when it answered something else. */
export function parseSession(body: unknown): { locked: boolean } | undefined {
  if (typeof body !== "object" || body === null) return undefined;
  const { required, signed_in: signedIn } = body as Record<string, unknown>;
  if (typeof required !== "boolean" || typeof signedIn !== "boolean") return undefined;
  return { locked: required && !signedIn };
}

/**
 * Asks the dashboard whether to show the sign-in screen. Never rejects: a
 * dashboard that cannot answer is treated as open, and the page's own
 * requests then report what is wrong.
 */
export async function check(): Promise<void> {
  try {
    const response = await send("/api/session", {
      cache: "no-store",
      signal: AbortSignal.timeout(20_000),
    });
    const session = response.ok ? parseSession(await response.json()) : undefined;
    gate.value = session?.locked ? "locked" : "open";
  } catch {
    gate.value = "open";
  }
}

/** Lets the page through after a sign-in the dashboard accepted. */
export function open(): void {
  gate.value = "open";
}

/**
 * Exchanges the token for the session cookie. Resolves to undefined when the
 * dashboard accepted it, otherwise to why it did not.
 */
export async function signIn(token: string): Promise<string | undefined> {
  let response: Response;
  try {
    response = await send("/api/session", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ token }),
      cache: "no-store",
      signal: AbortSignal.timeout(20_000),
    });
  } catch {
    return "Could not reach Mash. Check your connection, then try again.";
  }
  if (response.ok) return undefined;
  try {
    const failure: unknown = await response.json();
    const message = (failure as { error?: { message?: unknown } }).error?.message;
    if (typeof message === "string" && message !== "") return `${message}.`;
  } catch {
    /* A failure without a JSON body keeps the fallback below. */
  }
  return `Sign-in failed (${response.status}).`;
}
