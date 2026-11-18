import { useQuery } from "@tanstack/react-query";
import { createFileRoute, redirect, useNavigate } from "@tanstack/react-router";

import { StateCard } from "@/components/state-card";
import { Button } from "@/components/ui/button";
import { fetchHello, fetchMe, logout } from "@/lib/api";
import { clearToken, getToken } from "@/lib/session";

export const Route = createFileRoute("/")({
  // The guard (ADR-0004): the app's data is behind a token, so the app
  // itself is behind the login screen.
  beforeLoad: () => {
    if (!getToken()) {
      throw redirect({ to: "/login" });
    }
  },
  component: Home,
});

function Home() {
  const navigate = useNavigate();
  const hello = useQuery({ queryKey: ["hello"], queryFn: fetchHello });
  const me = useQuery({ queryKey: ["me"], queryFn: fetchMe });

  if (hello.isPending) {
    return <p className="text-muted-foreground">Loading…</p>;
  }

  if (hello.error) {
    return (
      <StateCard
        title="Something went wrong"
        description="Could not reach the upland API."
      />
    );
  }

  return (
    <StateCard title="Welcome" description="The API is connected.">
      <p>{hello.data.message}</p>
      {me.data ? (
        <p className="text-muted-foreground text-sm">
          Signed in as {me.data.username} ({me.data.email})
          {me.data.identities.length > 0
            ? ` · providers: ${me.data.identities.join(", ")}`
            : ""}
        </p>
      ) : null}
      <Button
        variant="outline"
        onClick={async () => {
          try {
            await logout();
          } catch {
            clearToken();
          }
          clearToken();
          await navigate({ to: "/login" });
        }}
      >
        Log out
      </Button>
    </StateCard>
  );
}
