import { createMemoryHistory, createRouter } from "@tanstack/react-router";

import { RouteError } from "./components/route-error.tsx";
import { routeTree } from "./routeTree.gen.ts";

export function createAppRouter(options?: { initialEntries?: string[] }) {
  const history = options?.initialEntries
    ? createMemoryHistory({ initialEntries: options.initialEntries })
    : undefined;

  return createRouter({
    routeTree,
    ...(history ? { history } : {}),
    defaultErrorComponent: RouteError,
  });
}
