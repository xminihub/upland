import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { Link, createFileRoute, useNavigate } from "@tanstack/react-router";

import { login } from "@/lib/api";
import { applyError, type FieldErrors } from "@/lib/error-copy";
import { flowCopy, listProviders, startLogin } from "@/lib/oidc";
import { setToken } from "@/lib/session";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Field,
  FieldError,
  FieldGroup,
  FieldLabel,
  FieldSeparator,
} from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Spinner } from "@/components/ui/spinner";

export const Route = createFileRoute("/login")({
  validateSearch: (search: Record<string, unknown>) => ({
    flow_error:
      typeof search.flow_error === "string" ? search.flow_error : undefined,
  }),
  component: Login,
});

function Login() {
  const navigate = useNavigate();
  const { flow_error: flowError } = Route.useSearch();
  const providers = useQuery({
    queryKey: ["providers"],
    queryFn: listProviders,
  });
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [pending, setPending] = useState(false);
  const [providerBusy, setProviderBusy] = useState<string | null>(null);
  const [fieldErrors, setFieldErrors] = useState<FieldErrors>({});
  const [formError, setFormError] = useState<string | null>(null);

  async function onSubmit(event: React.FormEvent) {
    event.preventDefault();
    setPending(true);
    setFieldErrors({});
    setFormError(null);
    try {
      const token = await login(username, password);
      setToken(token);
      await navigate({ to: "/" });
    } catch (error) {
      applyError(error, setFieldErrors, setFormError);
    } finally {
      setPending(false);
    }
  }

  return (
    <div className="grid place-content-center py-6">
      <Card className="w-full max-w-sm">
        <CardHeader>
          <CardTitle>Sign in</CardTitle>
          <CardDescription>
            Welcome back to upland, by password or by provider.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="flex flex-col gap-4">
            {providers.data && providers.data.length > 0 ? (
              <>
                {providers.data.map((provider) => (
                  <Button
                    key={provider.name}
                    type="button"
                    variant="outline"
                    disabled={providerBusy !== null}
                    onClick={async () => {
                      setProviderBusy(provider.name);
                      try {
                        await startLogin(provider);
                      } catch {
                        setProviderBusy(null);
                        setFormError("Could not reach the provider.");
                      }
                    }}
                  >
                    {providerBusy === provider.name ? (
                      <Spinner data-icon="inline-start" />
                    ) : null}
                    Sign in with {provider.name}
                  </Button>
                ))}
                <FieldSeparator>or</FieldSeparator>
              </>
            ) : null}
            <form onSubmit={onSubmit}>
              <FieldGroup>
                <Field data-invalid={Boolean(fieldErrors.username)}>
                  <FieldLabel htmlFor="login-username">Username</FieldLabel>
                  <Input
                    id="login-username"
                    aria-invalid={Boolean(fieldErrors.username)}
                    autoComplete="username"
                    value={username}
                    onChange={(event) => setUsername(event.target.value)}
                  />
                  <FieldError errors={toErrors(fieldErrors.username)} />
                </Field>
                <Field data-invalid={Boolean(fieldErrors.password)}>
                  <FieldLabel htmlFor="login-password">Password</FieldLabel>
                  <Input
                    id="login-password"
                    type="password"
                    aria-invalid={Boolean(fieldErrors.password)}
                    autoComplete="current-password"
                    value={password}
                    onChange={(event) => setPassword(event.target.value)}
                  />
                  <FieldError errors={toErrors(fieldErrors.password)} />
                </Field>
                {flowError ? (
                  <FieldError>{flowCopy(flowError)}</FieldError>
                ) : null}
                {formError ? <FieldError>{formError}</FieldError> : null}
                <Button type="submit" disabled={pending}>
                  {pending ? <Spinner data-icon="inline-start" /> : null}
                  {pending ? "Signing in…" : "Sign in"}
                </Button>
              </FieldGroup>
            </form>
          </div>
        </CardContent>
        <CardFooter className="text-muted-foreground text-sm">
          New here?{" "}
          <Link to="/signup" className="text-foreground underline">
            Create an account
          </Link>
        </CardFooter>
      </Card>
    </div>
  );
}

function toErrors(message?: string): Array<{ message?: string }> {
  return message ? [{ message }] : [];
}
