import { useState } from "react";
import { Link, createFileRoute, useNavigate } from "@tanstack/react-router";

import { signup } from "@/lib/api";
import { applyError, toErrors, type FieldErrors } from "@/lib/error-copy";
import { setToken } from "@/lib/access-token";
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
} from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Spinner } from "@/components/ui/spinner";

export const Route = createFileRoute("/signup")({ component: Signup });

function Signup() {
  const navigate = useNavigate();
  const [username, setUsername] = useState("");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [pending, setPending] = useState(false);
  const [fieldErrors, setFieldErrors] = useState<FieldErrors>({});
  const [formError, setFormError] = useState<string | null>(null);

  async function onSubmit(event: React.FormEvent) {
    event.preventDefault();
    setPending(true);
    setFieldErrors({});
    setFormError(null);
    try {
      const token = await signup(username, email, password);
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
          <CardTitle>Create an account</CardTitle>
          <CardDescription>
            Pick a username, claim an email, set a password.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <form onSubmit={onSubmit}>
            <FieldGroup>
              <Field data-invalid={Boolean(fieldErrors.username)}>
                <FieldLabel htmlFor="signup-username">Username</FieldLabel>
                <Input
                  id="signup-username"
                  aria-invalid={Boolean(fieldErrors.username)}
                  autoComplete="username"
                  value={username}
                  onChange={(event) => setUsername(event.target.value)}
                />
                <FieldError errors={toErrors(fieldErrors.username)} />
              </Field>
              <Field data-invalid={Boolean(fieldErrors.email)}>
                <FieldLabel htmlFor="signup-email">Email</FieldLabel>
                <Input
                  id="signup-email"
                  type="email"
                  aria-invalid={Boolean(fieldErrors.email)}
                  autoComplete="email"
                  value={email}
                  onChange={(event) => setEmail(event.target.value)}
                />
                <FieldError errors={toErrors(fieldErrors.email)} />
              </Field>
              <Field data-invalid={Boolean(fieldErrors.password)}>
                <FieldLabel htmlFor="signup-password">Password</FieldLabel>
                <Input
                  id="signup-password"
                  type="password"
                  aria-invalid={Boolean(fieldErrors.password)}
                  autoComplete="new-password"
                  value={password}
                  onChange={(event) => setPassword(event.target.value)}
                />
                <FieldError errors={toErrors(fieldErrors.password)} />
              </Field>
              {formError ? <FieldError>{formError}</FieldError> : null}
              <Button type="submit" disabled={pending}>
                {pending ? <Spinner data-icon="inline-start" /> : null}
                {pending ? "Creating…" : "Create account"}
              </Button>
            </FieldGroup>
          </form>
        </CardContent>
        <CardFooter className="text-muted-foreground text-sm">
          Already have one?{" "}
          <Link to="/login" className="text-foreground underline">
            Sign in
          </Link>
        </CardFooter>
      </Card>
    </div>
  );
}
