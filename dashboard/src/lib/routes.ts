// What each route answers with, taken from the contract the server generates
// (`openapi.json`, turned into `contract.ts` by `pnpm contract`).
import type { paths } from "./contract";

/** A route that is read with GET. */
export type ReadPath = {
  [P in keyof paths]: paths[P] extends { get: { responses: { 200: unknown } } } ? P : never;
}[keyof paths];

/** The body a successful GET of `P` returns. */
export type Answer<P extends ReadPath> =
  paths[P]["get"]["responses"][200]["content"]["application/json"];
