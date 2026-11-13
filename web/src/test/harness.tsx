import { QueryClient } from "@tanstack/react-query";
import { render } from "@testing-library/react";

import { App } from "@/app";
import { createAppRouter } from "@/router";

export function renderApp(path = "/") {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const router = createAppRouter({ initialEntries: [path] });

  return render(<App router={router} queryClient={queryClient} />);
}
