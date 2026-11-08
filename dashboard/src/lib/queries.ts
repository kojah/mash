import { queryOptions } from "@tanstack/svelte-query";
import { catalog } from "./api";

export const catalogOptions = queryOptions({
  queryKey: ["catalog"],
  queryFn: ({ signal }) => catalog(signal),
  staleTime: Infinity,
  retry: false,
  refetchOnMount: false,
  refetchOnWindowFocus: false,
  refetchOnReconnect: false,
});
/** Submitted searches and reading pages change only at the reader's request. */
export const stableRead = {
  // Explicit reads must settle even when navigator.onLine is false. Otherwise
  // TanStack pauses them before fetch and the screen stays "Loading" forever.
  networkMode: "always",
  staleTime: Infinity,
  refetchOnWindowFocus: false,
  refetchOnReconnect: false,
  refetchOnMount: false,
  retry: false,
  refetchInterval: false,
} as const;
