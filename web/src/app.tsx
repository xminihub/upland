import { QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider } from "@tanstack/react-router";
import { ReactQueryDevtools } from "@tanstack/react-query-devtools";
import { TanStackRouterDevtools } from "@tanstack/react-router-devtools";

import type { QueryClient } from "@tanstack/react-query";
import type { AnyRouter } from "@tanstack/react-router";

export function App({
  router,
  queryClient,
  devtools = false,
}: {
  router: AnyRouter;
  queryClient: QueryClient;
  devtools?: boolean;
}) {
  return (
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
      {devtools ? (
        <>
          <TanStackRouterDevtools router={router} />
          <ReactQueryDevtools initialIsOpen={false} />
        </>
      ) : null}
    </QueryClientProvider>
  );
}
