import { queryOptions } from "@tanstack/svelte-query";
import { catalog } from "./api";

export const catalogOptions = queryOptions({
  queryKey: ["catalog"],
  queryFn: ({ signal }) => catalog(signal),
  staleTime: 15_000,
  refetchInterval: 30_000,
  refetchIntervalInBackground: false,
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
} as const;
