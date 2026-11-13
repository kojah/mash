// What each route reads and answers with, taken from the contract the server
// generates (`openapi.json`, turned into `contract.ts` by `pnpm contract`).
import type { components, paths } from "./contract";

/** The types the server sends and accepts, by their names in the contract. */
export type Wire = components["schemas"];

/** A route that is read with GET and answers with JSON; the event stream is not one. */
export type ReadPath = {
  [P in keyof paths]: paths[P] extends {
    get: { responses: { 200: { content: { "application/json": unknown } } } };
  }
    ? P
    : never;
}[keyof paths];

/** The body a successful GET of `P` returns. */
export type Answer<P extends ReadPath> =
  paths[P]["get"]["responses"][200]["content"]["application/json"];

/** A route that is changed with POST. */
export type WritePath = {
  [P in keyof paths]: paths[P] extends {
    post: { responses: { 200: { content: { "application/json": unknown } } } };
  }
    ? P
    : never;
}[keyof paths];

/** The body a POST to `P` sends. */
export type Sent<P extends WritePath> =
  paths[P]["post"]["requestBody"]["content"]["application/json"];

/** The body a successful POST to `P` returns. */
export type Changed<P extends WritePath> =
  paths[P]["post"]["responses"][200]["content"]["application/json"];
