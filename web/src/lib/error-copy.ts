// The copy the forms speak (spec #13): every stable API error code has a
// sentence, so the UI branches on the code and never on parsing English.

import { ApiRequestError } from "./api";

export type FieldErrors = Record<string, string>;

const FIELD_COPY: Record<string, string> = {
  username_taken: "That username is taken.",
  email_taken: "That email already belongs to an account.",
  unauthorized: "Username or password is wrong.",
  provider_unknown: "That sign-in method is not configured.",
  oidc_rejected: "The provider's sign-in response was not accepted.",
  provider_no_email: "The provider did not share an email address.",
};

// The one home of the API-code copy, so the browser-side flow (oidc.ts)
// can quote it without restating a sentence two places must keep in step.
export function apiCopy(code: string): string | undefined {
  return FIELD_COPY[code];
}

// shadcn's FieldError takes a list of problems; a form holds at most one
// per field, and "no message" is the empty list.
export function toErrors(message?: string): Array<{ message?: string }> {
  return message ? [{ message }] : [];
}

const RULES: Record<string, Record<string, string>> = {
  username: {
    invalid:
      "Use 3 to 32 characters: letters, digits, dots, dashes, underscores.",
  },
  email: {
    invalid: "An address with one @ and text on both sides.",
  },
  password: {
    invalid: "A password of at least one, and no more than 72, bytes.",
  },
};

export function messageFor(error: ApiRequestError): {
  field?: string;
  message: string;
} {
  if (error.code === "invalid") {
    if (error.field && RULES[error.field]?.invalid) {
      return { field: error.field, message: RULES[error.field].invalid };
    }
    return { message: "Check the highlighted fields." };
  }
  return {
    field: error.field ?? undefined,
    message: FIELD_COPY[error.code] ?? "Something went wrong.",
  };
}

export function applyError(
  error: unknown,
  setFieldErrors: (errors: FieldErrors) => void,
  setFormError: (message: string | null) => void,
): void {
  if (error instanceof ApiRequestError) {
    const { field, message } = messageFor(error);
    if (field) {
      setFieldErrors({ [field]: message });
    } else {
      setFieldErrors({});
      setFormError(message);
    }
    return;
  }
  setFieldErrors({});
  setFormError("Could not reach the upland API.");
}
