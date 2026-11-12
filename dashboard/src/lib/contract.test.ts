import { readFile } from "node:fs/promises";
import openapiTS, { astToString } from "openapi-typescript";
import { expect, it } from "vitest";

// `contract.ts` is generated; a stale copy would type-check requests against
// a contract the server no longer keeps.
it("the generated types match openapi.json", async () => {
  const document = new URL("../../openapi.json", import.meta.url);
  const generated = astToString(await openapiTS(document));
  const committed = await readFile(new URL("./contract.ts", import.meta.url), "utf8");
  // The command-line generator adds a header the library call does not.
  const body = committed.replace(/^\/\*\*[\s\S]*?\*\/\n\n/, "");
  expect(body, "run `pnpm contract` in dashboard/").toBe(generated);
});
