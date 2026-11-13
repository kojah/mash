import { afterEach, describe, expect, it } from "vitest";
import { check, gate, linkToken, open, parseSession, signIn } from "./session.svelte";
import { install, type Transport } from "./transport";

const json = (status: number, body: unknown) => new Response(JSON.stringify(body), { status });

afterEach(() => {
  install((path, init) => fetch(path, init));
  gate.value = "checking";
});

describe("the dashboard's sign-in gate", () => {
  it("locks only when a sign-in is required and missing", () => {
    expect(parseSession({ required: true, signed_in: false })).toEqual({ locked: true });
    expect(parseSession({ required: true, signed_in: true })).toEqual({ locked: false });
    expect(parseSession({ required: false, signed_in: true })).toEqual({ locked: false });
    for (const body of [null, "locked", {}, { required: "yes", signed_in: false }])
      expect(parseSession(body)).toBeUndefined();
  });

  it("shows the sign-in for a locked dashboard and the page otherwise", async () => {
    install(async () => json(200, { required: true, signed_in: false }));
    await check();
    expect(gate.value).toBe("locked");
    open();
    expect(gate.value).toBe("open");
    // A dashboard that cannot answer is left to its own requests to explain.
    const unanswered: Transport[] = [
      async () => json(404, { error: { message: "not in the demo" } }),
      async () => json(200, { unexpected: true }),
      async () => {
        throw new TypeError("offline");
      },
    ];
    for (const transport of unanswered) {
      gate.value = "checking";
      install(transport);
      await check();
      expect(gate.value).toBe("open");
    }
  });

  it("reads a token only from a whole sign-in link", () => {
    const token = "0123456789abcdef0123456789abcdef";
    expect(linkToken(`#signin=${token}`)).toBe(token);
    for (const hash of [
      "",
      "#/",
      `#/search?q=signin=${token}`,
      "#signin=short",
      `#signin=${token} x`,
    ])
      expect(linkToken(hash)).toBeUndefined();
  });

  it("sends the token once and reports why it was refused", async () => {
    const sent: { path: string; init: RequestInit }[] = [];
    install(async (path, init) => {
      sent.push({ path, init });
      return json(200, { required: true, signed_in: true });
    });
    expect(await signIn("a-long-access-token")).toBeUndefined();
    expect(sent).toHaveLength(1);
    expect(sent[0]?.path).toBe("/api/session");
    expect(sent[0]?.init.method).toBe("POST");
    expect(sent[0]?.init.body).toBe(JSON.stringify({ token: "a-long-access-token" }));

    install(async () => json(401, { error: { message: "That access token is not right" } }));
    expect(await signIn("wrong")).toBe("That access token is not right.");
    install(async () => new Response("busy", { status: 503 }));
    expect(await signIn("wrong")).toBe("Sign-in failed (503).");
    install(async () => {
      throw new TypeError("offline");
    });
    expect(await signIn("wrong")).toContain("Could not reach Mash");
  });
});
