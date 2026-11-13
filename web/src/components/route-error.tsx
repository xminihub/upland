import { StateCard } from "@/components/state-card";

export function RouteError({ error }: { error: unknown }) {
  const message =
    error instanceof Error && error.message ? error.message : "Unknown error";

  return (
    <StateCard
      title="Something went wrong"
      description={`The page failed to load (${message}).`}
    />
  );
}
