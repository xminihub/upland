import { fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { OIDC_PENDING_KEY } from "@/lib/oidc";
import { setToken } from "@/lib/access-token";
import { renderApp } from "@/test/harness";

function mockApi(handlers: Record<string, () => Response>) {
  const fetchMock = vi.fn(
    async (input: string | URL | Request, _init?: RequestInit) => {
      const url = String(input).split("?")[0];
      const key = Object.keys(handlers).find((candidate) =>
        url.endsWith(candidate),
      );
      if (!key) {
        throw new Error(`unexpected fetch: ${url}`);
      }
      return handlers[key]();
    },
  );
  vi.stubGlobal("fetch", fetchMock);
  return fetchMock;
}

function findSignInTitle() {
  return screen.findByText("Sign in", {
    selector: "[data-slot=card-title]",
  });
}

function findSignInTitleSync() {
  return screen.getByText("Sign in", {
    selector: "[data-slot=card-title]",
  });
}

async function submit(buttonName: string) {
  fireEvent.click(screen.getByRole("button", { name: buttonName }));
}

beforeEach(() => {
  localStorage.clear();
  sessionStorage.clear();
  vi.unstubAllGlobals();
});

describe("Login screen", () => {
  it("greets the unauthenticated visitor instead of the app", async () => {
    mockApi({});
    renderApp("/");

    expect(await findSignInTitle()).toBeInTheDocument();
    expect(screen.getByLabelText("Username")).toBeInTheDocument();
    expect(screen.getByLabelText("Password")).toBeInTheDocument();
  });

  it("signs in and enters the app, keeping the token", async () => {
    mockApi({
      "/api/v1/auth/login": () =>
        Response.json({ access_token: "issued-token" }),
      "/api/v1/hello": () => Response.json({ message: "Hello from upland!" }),
      "/api/v1/auth/me": () =>
        Response.json({
          username: "reta",
          email: "reta@example.com",
          identities: [],
        }),
    });
    renderApp("/login");
    await screen.findByLabelText("Username");

    fireEvent.change(screen.getByLabelText("Username"), {
      target: { value: "Reta" },
    });
    fireEvent.change(screen.getByLabelText("Password"), {
      target: { value: "hunter2" },
    });
    submit("Sign in");

    expect(await screen.findByText("Hello from upland!")).toBeInTheDocument();
    expect(localStorage.getItem("upland_token")).toBe("issued-token");
  });

  it("names the failure without leaking which half was wrong", async () => {
    mockApi({
      "/api/v1/auth/login": () =>
        Response.json({ error: { code: "unauthorized" } }, { status: 401 }),
    });
    renderApp("/login");
    await screen.findByLabelText("Username");

    fireEvent.change(screen.getByLabelText("Username"), {
      target: { value: "nobody" },
    });
    fireEvent.change(screen.getByLabelText("Password"), {
      target: { value: "guess" },
    });
    submit("Sign in");

    expect(
      await screen.findByText("Username or password is wrong."),
    ).toBeInTheDocument();
    expect(await findSignInTitle()).toBeInTheDocument();
    expect(localStorage.getItem("upland_token")).toBeNull();
  });
});

describe("Sign up screen", () => {
  it("maps taken handles onto the fields that collided", async () => {
    mockApi({
      "/api/v1/auth/signup": () =>
        Response.json(
          { error: { code: "username_taken", field: "username" } },
          { status: 409 },
        ),
    });
    renderApp("/signup");
    await screen.findByLabelText("Username");

    fireEvent.change(screen.getByLabelText("Username"), {
      target: { value: "taken-one" },
    });
    fireEvent.change(screen.getByLabelText("Email"), {
      target: { value: "me@example.com" },
    });
    fireEvent.change(screen.getByLabelText("Password"), {
      target: { value: "hunter2" },
    });
    submit("Create account");

    expect(
      await screen.findByText("That username is taken."),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("Username")).toHaveAttribute(
      "aria-invalid",
      "true",
    );
  });

  it("lands a fresh account straight inside the app", async () => {
    mockApi({
      "/api/v1/auth/signup": () => Response.json({ access_token: "new-token" }),
      "/api/v1/hello": () => Response.json({ message: "Hello from upland!" }),
      "/api/v1/auth/me": () =>
        Response.json({
          username: "fresh_one",
          email: "fresh@example.com",
          identities: [],
        }),
    });
    renderApp("/signup");
    await screen.findByLabelText("Username");

    fireEvent.change(screen.getByLabelText("Username"), {
      target: { value: "fresh_one" },
    });
    fireEvent.change(screen.getByLabelText("Email"), {
      target: { value: "fresh@example.com" },
    });
    fireEvent.change(screen.getByLabelText("Password"), {
      target: { value: "hunter2" },
    });
    submit("Create account");

    expect(await screen.findByText("Hello from upland!")).toBeInTheDocument();
  });
});

describe("The signed-in browser", () => {
  beforeEach(() => {
    setToken("stored-token");
  });

  it("shows the account the token was minted for", async () => {
    mockApi({
      "/api/v1/hello": () => Response.json({ message: "Hello from upland!" }),
      "/api/v1/auth/me": () =>
        Response.json({
          username: "tess",
          email: "tess@example.com",
          identities: ["work"],
        }),
    });
    renderApp("/");

    expect(await screen.findByText("Hello from upland!")).toBeInTheDocument();
    expect(screen.getByText(/tess@example\.com/)).toBeInTheDocument();
  });

  it("brings an expired access token back to the login screen", async () => {
    mockApi({
      "/api/v1/hello": () =>
        Response.json({ error: { code: "unauthorized" } }, { status: 401 }),
      "/api/v1/auth/me": () =>
        Response.json({ error: { code: "unauthorized" } }, { status: 401 }),
    });
    renderApp("/");

    expect(await findSignInTitle()).toBeInTheDocument();
    expect(localStorage.getItem("upland_token")).toBeNull();
  });

  it("logout forgets the token and returns to the login screen", async () => {
    mockApi({
      "/api/v1/hello": () => Response.json({ message: "Hello from upland!" }),
      "/api/v1/auth/me": () =>
        Response.json({
          username: "tess",
          email: "tess@example.com",
          identities: [],
        }),
      "/api/v1/auth/logout": () => Response.json({ ok: true }),
    });
    renderApp("/");

    await screen.findByText("Hello from upland!");
    fireEvent.click(screen.getByRole("button", { name: /log out/i }));

    await waitFor(() => {
      expect(findSignInTitleSync()).toBeInTheDocument();
    });
    expect(localStorage.getItem("upland_token")).toBeNull();
  });

  it("attaches the bearer header to every call", async () => {
    const fetchMock = mockApi({
      "/api/v1/hello": () => Response.json({ message: "Hello from upland!" }),
      "/api/v1/auth/me": () =>
        Response.json({
          username: "tess",
          email: "tess@example.com",
          identities: [],
        }),
    });
    renderApp("/");

    await screen.findByText("Hello from upland!");
    const helloCall = fetchMock.mock.calls.find(([url]) =>
      String(url).endsWith("/api/v1/hello"),
    );
    const headers = new Headers(helloCall![1]?.headers);
    expect(headers.get("authorization")).toBe("Bearer stored-token");
  });
});

describe("Unknown routes", () => {
  it("shows a not-found view for any unknown path", async () => {
    mockApi({});
    renderApp("/does-not-exist");

    expect(await screen.findByText("Page not found")).toBeInTheDocument();
    expect(screen.getByRole("banner")).toHaveTextContent("Upland");
  });
});

// The OIDC door (#25): provider buttons, the hand-rolled PKCE module,
// and the callback that trades the code for an upland token.

function stubCrypto() {
  vi.stubGlobal("crypto", {
    getRandomValues: (array: Uint8Array) => {
      for (let i = 0; i < array.length; i++) array[i] = i + 1;
      return array;
    },
    subtle: {
      digest: async () => new Uint8Array([1, 2, 3]).buffer,
    },
  });
}

const PROVIDERS = [
  {
    name: "work",
    issuer: "https://issuer.test/realms/main",
    client_id: "upland-work",
  },
];

describe("OIDC sign-in", () => {
  it("offers one button per configured provider", async () => {
    stubCrypto();
    mockApi({
      "/api/v1/auth/providers": () => Response.json(PROVIDERS),
    });
    renderApp("/login");
    await screen.findByLabelText("Username");

    expect(
      await screen.findByRole("button", { name: "Sign in with work" }),
    ).toBeInTheDocument();
  });

  it("hides the door entirely when no provider is configured", async () => {
    mockApi({
      "/api/v1/auth/providers": () => Response.json([]),
    });
    renderApp("/login");
    await screen.findByLabelText("Username");

    expect(
      screen.queryByRole("button", { name: /Sign in with/ }),
    ).not.toBeInTheDocument();
  });

  it("starts PKCE against the issuer and hands the browser over", async () => {
    stubCrypto();
    const assign = vi.fn();
    vi.stubGlobal("location", { origin: "http://localhost", assign });
    mockApi({
      "/api/v1/auth/providers": () => Response.json(PROVIDERS),
      "/.well-known/openid-configuration": () =>
        Response.json({
          authorization_endpoint: "https://issuer.test/authorize",
          token_endpoint: "http://localhost/token",
        }),
    });
    renderApp("/login");
    await screen.findByLabelText("Username");

    fireEvent.click(
      await screen.findByRole("button", { name: "Sign in with work" }),
    );

    await waitFor(() => expect(assign).toHaveBeenCalled());
    const url = new URL(assign.mock.calls[0][0]);
    expect(url.origin + url.pathname).toBe("https://issuer.test/authorize");
    expect(url.searchParams.get("response_type")).toBe("code");
    expect(url.searchParams.get("code_challenge_method")).toBe("S256");
    expect(url.searchParams.get("code_challenge")).toBe("AQID");
    expect(url.searchParams.get("scope")).toBe("openid email");
    expect(url.searchParams.get("redirect_uri")).toBe(
      "http://localhost/oauth/callback",
    );
    expect(url.searchParams.get("state")).toBeTruthy();
    expect(sessionStorage.getItem(OIDC_PENDING_KEY)).toContain("verifier");
  });

  it("trades the callback code for an upland token", async () => {
    vi.stubGlobal("location", { origin: "http://localhost", assign: vi.fn() });
    sessionStorage.setItem(
      OIDC_PENDING_KEY,
      JSON.stringify({
        provider: "work",
        state: "GOODSTATE",
        verifier: "the-verifier",
        nonce: "the-nonce",
        token_endpoint: "http://localhost/token",
        client_id: "upland-work",
      }),
    );
    mockApi({
      "/token": () =>
        Response.json({ id_token: "ID-TOKEN", token_type: "Bearer" }),
      "/api/v1/auth/oidc": () => Response.json({ access_token: "AT" }),
      "/api/v1/hello": () => Response.json({ message: "Hello from upland!" }),
      "/api/v1/auth/me": () =>
        Response.json({
          username: "tess",
          email: "tess@example.com",
          identities: ["work"],
        }),
    });
    renderApp("/oauth/callback?code=THECODE&state=GOODSTATE");

    expect(await screen.findByText("Hello from upland!")).toBeInTheDocument();
    expect(localStorage.getItem("upland_token")).toBe("AT");
    expect(sessionStorage.getItem(OIDC_PENDING_KEY)).toBeNull();
  });

  it("refuses a state that does not match the sign-in started", async () => {
    vi.stubGlobal("location", { origin: "http://localhost", assign: vi.fn() });
    sessionStorage.setItem(
      OIDC_PENDING_KEY,
      JSON.stringify({
        provider: "work",
        state: "GOODSTATE",
        verifier: "the-verifier",
        nonce: "the-nonce",
        token_endpoint: "http://localhost/token",
        client_id: "upland-work",
      }),
    );
    mockApi({});
    renderApp("/oauth/callback?code=THECODE&state=WRONGSTATE");

    expect(
      await screen.findByText(
        "The provider's response did not match the sign-in you started.",
      ),
    ).toBeInTheDocument();
    expect(localStorage.getItem("upland_token")).toBeNull();
  });

  it("reports a provider refusal by name on the login screen", async () => {
    vi.stubGlobal("location", { origin: "http://localhost", assign: vi.fn() });
    mockApi({});
    renderApp("/oauth/callback?error=access_denied&state=GOODSTATE");

    expect(
      await screen.findByText("The provider refused the sign-in."),
    ).toBeInTheDocument();
  });
});
