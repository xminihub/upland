// Every call to the API goes through here: the bearer header when there
// is a token, the structured error body when the answer is not OK, and a
// 401 anywhere forgetting the token and heading back to the login screen
// — the arc the route guard then shows.

import { clearToken, getToken } from "./access-token.ts";

export type ApiError = {
  status: number;
  code: string;
  field?: string;
};

export class ApiRequestError extends Error implements ApiError {
  status: number;
  code: string;
  field?: string;

  constructor(status: number, code: string, field?: string) {
    super(code);
    this.status = status;
    this.code = code;
    this.field = field;
  }
}

export const UNAUTHORIZED_EVENT = "upland:unauthorized";

function parseError(status: number, body: unknown): ApiRequestError {
  const error = (body as { error?: { code?: string; field?: string } })?.error;
  return new ApiRequestError(
    status,
    error?.code ?? "request_failed",
    error?.field,
  );
}

export async function apiRequest<T>(
  path: string,
  init: RequestInit = {},
): Promise<T> {
  const headers = new Headers(init.headers);
  const token = getToken();
  if (token) {
    headers.set("authorization", `Bearer ${token}`);
  }

  const response = await fetch(path, { ...init, headers });

  if (response.status === 401) {
    clearToken();
    window.dispatchEvent(new Event(UNAUTHORIZED_EVENT));
  }

  if (!response.ok) {
    let body: unknown = null;
    try {
      body = await response.clone().json();
    } catch {
      body = null;
    }
    throw parseError(response.status, body);
  }

  return (await response.json()) as T;
}

export type TokenResponse = { access_token: string };
export type MeResponse = {
  id: number;
  username: string;
  email: string;
  identities: string[];
};
export type HelloResponse = { message: string };

export async function fetchHello(): Promise<HelloResponse> {
  return apiRequest<HelloResponse>("/api/v1/hello");
}

export async function fetchMe(): Promise<MeResponse> {
  return apiRequest<MeResponse>("/api/v1/auth/me");
}

export async function login(
  username: string,
  password: string,
): Promise<string> {
  const answer = await apiRequest<TokenResponse>("/api/v1/auth/login", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ username, password }),
  });
  return answer.access_token;
}

export async function signup(
  username: string,
  email: string,
  password: string,
): Promise<string> {
  const answer = await apiRequest<TokenResponse>("/api/v1/auth/signup", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ username, email, password }),
  });
  return answer.access_token;
}

export async function logout(): Promise<void> {
  await apiRequest("/api/v1/auth/logout", { method: "POST" });
  clearToken();
}
