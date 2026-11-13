import { createFileRoute } from "@tanstack/react-router";

import { Button } from "@/components/ui/button";
import { StateCard } from "@/components/state-card";

export const Route = createFileRoute("/$")({ component: NotFound });

function NotFound() {
  return (
    <StateCard
      title="Page not found"
      description="The page you are looking for does not exist."
    >
      <Button asChild>
        <a href="/">Go home</a>
      </Button>
    </StateCard>
  );
}
