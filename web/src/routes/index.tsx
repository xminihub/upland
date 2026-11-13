import { useQuery } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";

import { StateCard } from "@/components/state-card";
import { fetchHello } from "@/lib/api";

export const Route = createFileRoute("/")({ component: Home });

function Home() {
  const { data, isPending, error } = useQuery({
    queryKey: ["hello"],
    queryFn: fetchHello,
  });

  if (isPending) {
    return <p className="text-muted-foreground">Loading…</p>;
  }

  if (error) {
    return (
      <StateCard
        title="Something went wrong"
        description="Could not reach the upland API."
      />
    );
  }

  return (
    <StateCard title="Welcome" description="The API is connected.">
      <p>{data.message}</p>
    </StateCard>
  );
}
