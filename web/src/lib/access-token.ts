// The access token's home (ADR-0004): localStorage, because bearer-only
// means the browser is the store and a reload must not mean a logout.

const TOKEN_KEY = "upland_token";

export function getToken(): string | null {
  return localStorage.getItem(TOKEN_KEY);
}

export function setToken(token: string): void {
  localStorage.setItem(TOKEN_KEY, token);
}

export function clearToken(): void {
  localStorage.removeItem(TOKEN_KEY);
}
