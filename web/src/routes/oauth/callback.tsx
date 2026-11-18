import { useEffect } from "react";
import { createFileRoute, useNavigate } from "@tanstack/react-router";

import { setToken } from "@/lib/access-token";
import { errorCode, finishLogin } from "@/lib/oidc";

export const Route = createFileRoute("/oauth/callback")({
  validateSearch: (search: Record<string, unknown>) => ({
    code: typeof search.code === "string" ? search.code : "",
    state: typeof search.state === "string" ? search.state : "",
    error: typeof search.error === "string" ? search.error : "",
  }),
  component: Callback,
});

function Callback() {
  const search = Route.useSearch();
  const navigate = useNavigate();

  // The IdP's bounce back: one shot at the trade, then the login screen
  // either holds the access token or names what went wrong (ADR-0004).
  const code = search.code;
  const state = search.state;
  const error = search.error;
  useEffect(() => {
    let cancelled = false;
    const params = new URLSearchParams();
    for (const [key, value] of [
      ["code", code],
      ["state", state],
      ["error", error],
    ] as const) {
      if (value) params.set(key, value);
    }

    void (async () => {
      try {
        const token = await finishLogin(params);
        if (cancelled) return;
        setToken(token);
        await navigate({ to: "/" });
      } catch (thrown) {
        if (cancelled) return;
        await navigate({
          to: "/login",
          search: { flow_error: errorCode(thrown) },
        });
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [navigate, code, state, error]);

  return <p className="text-muted-foreground">Finishing sign-in…</p>;
}
