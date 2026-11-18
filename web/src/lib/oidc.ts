// The browser-side OIDC client (ADR-0004): discovery, PKCE, the code
// exchange. Deliberately minimal — this module relays the ID token to the
// backend and never trusts it; real verification is the backend's job.

import { ApiRequestError, apiRequest } from "./api.ts";
import { apiCopy } from "./error-copy";

export const OIDC_PENDING_KEY = "upland_oidc_pending";
export const OIDC_CALLBACK_PATH = "/oauth/callback";

export type OidcProviderListing = {
  name: string;
  issuer: string;
  client_id: string;
};

type PendingLogin = {
  provider: string;
  state: string;
  verifier: string;
  nonce: string;
  token_endpoint: string;
  client_id: string;
};

export class OidcFlowError extends Error {}

function base64Url(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary)
    .replaceAll("+", "-")
    .replaceAll("/", "_")
    .replaceAll("=", "");
}

export function randomSecret(length = 16): string {
  const bytes = new Uint8Array(length);
  crypto.getRandomValues(bytes);
  return base64Url(bytes);
}

async function pkceChallenge(verifier: string): Promise<string> {
  const digest = await crypto.subtle.digest(
    "SHA-256",
    new TextEncoder().encode(verifier),
  );
  return base64Url(new Uint8Array(digest));
}

async function discover(issuer: string): Promise<Record<string, string>> {
  const url = `${issuer.replace(/\/+$/, "")}/.well-known/openid-configuration`;
  const response = await fetch(url);
  if (!response.ok) {
    throw new OidcFlowError(
      "The provider's discovery document was unreachable.",
    );
  }
  return (await response.json()) as Record<string, string>;
}

export async function listProviders(): Promise<OidcProviderListing[]> {
  return apiRequest<OidcProviderListing[]>("/api/v1/auth/providers");
}

export async function startLogin(provider: OidcProviderListing): Promise<void> {
  const metadata = await discover(provider.issuer);
  const state = randomSecret();
  const nonce = randomSecret();
  const verifier = randomSecret(32);
  const redirectUri = `${window.location.origin}${OIDC_CALLBACK_PATH}`;

  sessionStorage.setItem(
    OIDC_PENDING_KEY,
    JSON.stringify({
      provider: provider.name,
      state,
      verifier,
      nonce,
      token_endpoint: metadata["token_endpoint"],
      client_id: provider.client_id,
    } satisfies PendingLogin),
  );

  const authorize = new URL(metadata["authorization_endpoint"]);
  authorize.searchParams.set("response_type", "code");
  authorize.searchParams.set("client_id", provider.client_id);
  authorize.searchParams.set("redirect_uri", redirectUri);
  authorize.searchParams.set("scope", "openid email");
  authorize.searchParams.set("state", state);
  authorize.searchParams.set("nonce", nonce);
  authorize.searchParams.set("code_challenge", await pkceChallenge(verifier));
  authorize.searchParams.set("code_challenge_method", "S256");
  window.location.assign(authorize.toString());
}

export async function finishLogin(params: URLSearchParams): Promise<string> {
  if (params.get("error")) {
    throw new OidcFlowError("provider_refused");
  }

  const raw = sessionStorage.getItem(OIDC_PENDING_KEY);
  if (!raw) {
    throw new OidcFlowError("state_missing");
  }
  const pending = JSON.parse(raw) as PendingLogin;
  if (params.get("state") !== pending.state) {
    sessionStorage.removeItem(OIDC_PENDING_KEY);
    throw new OidcFlowError("state_mismatch");
  }

  const code = params.get("code");
  if (!code) {
    throw new OidcFlowError("state_missing");
  }

  const tokenResponse = await fetch(pending.token_endpoint, {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({
      grant_type: "authorization_code",
      code,
      client_id: pending.client_id,
      redirect_uri: `${window.location.origin}${OIDC_CALLBACK_PATH}`,
      code_verifier: pending.verifier,
    }),
  });
  if (!tokenResponse.ok) {
    sessionStorage.removeItem(OIDC_PENDING_KEY);
    throw new OidcFlowError("exchange_refused");
  }
  const tokens = (await tokenResponse.json()) as { id_token?: string };
  if (!tokens.id_token) {
    throw new OidcFlowError("exchange_refused");
  }

  const answer = await apiRequest<{ access_token: string }>(
    "/api/v1/auth/oidc",
    {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        provider: pending.provider,
        id_token: tokens.id_token,
        nonce: pending.nonce,
      }),
    },
  );
  sessionStorage.removeItem(OIDC_PENDING_KEY);
  return answer.access_token;
}

export function flowCopy(code: string): string {
  switch (code) {
    case "state_mismatch":
      return "The provider's response did not match the sign-in you started.";
    case "state_missing":
      return "There is no sign-in in progress to finish.";
    case "provider_refused":
      return "The provider refused the sign-in.";
    case "exchange_refused":
      return "The provider would not trade the code for a token.";
    default:
      // A code the backend answered with is the forms' copy to give;
      // anything else means the issuer itself never spoke.
      return apiCopy(code) ?? "Could not reach the provider.";
  }
}

export function errorCode(error: unknown): string {
  if (error instanceof OidcFlowError) return error.message;
  if (error instanceof ApiRequestError) return error.code;
  return "unreachable";
}

export function oidcFlowMessage(error: unknown): string {
  return flowCopy(errorCode(error));
}
