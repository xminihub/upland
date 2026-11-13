import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { QueryClient } from "@tanstack/react-query";

import "./index.css";
import { App } from "./app.tsx";
import { createAppRouter } from "./router.ts";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App
      router={createAppRouter()}
      queryClient={new QueryClient()}
      devtools={import.meta.env.DEV}
    />
  </StrictMode>,
);
