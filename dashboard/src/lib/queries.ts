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
  staleTime: Infinity,
  refetchOnWindowFocus: false,
  refetchOnReconnect: false,
  refetchOnMount: false,
  retry: false,
  refetchInterval: false,
} as const;
