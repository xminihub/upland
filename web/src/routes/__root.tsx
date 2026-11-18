import { useEffect } from "react";
import {
  Link,
  Outlet,
  createRootRoute,
  useNavigate,
} from "@tanstack/react-router";

import { UNAUTHORIZED_EVENT } from "@/lib/api";

export const Route = createRootRoute({ component: RootLayout });

function RootLayout() {
  const navigate = useNavigate();

  // A 401 anywhere is the session ending (ADR-0004): the API layer has
  // already forgotten the token, this decides the screen does too.
  useEffect(() => {
    const forget = () => {
      void navigate({ to: "/login" });
    };
    window.addEventListener(UNAUTHORIZED_EVENT, forget);
    return () => window.removeEventListener(UNAUTHORIZED_EVENT, forget);
  }, [navigate]);

  return (
    <div className="flex min-h-dvh flex-col">
      <header className="border-b">
        <div className="mx-auto flex w-full max-w-3xl items-center px-6 py-4">
          <Link to="/" className="font-semibold">
            Upland
          </Link>
        </div>
      </header>
      <main className="mx-auto w-full max-w-3xl flex-1 px-6 py-12">
        <Outlet />
      </main>
    </div>
  );
}
