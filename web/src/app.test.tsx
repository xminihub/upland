import { fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { setToken } from "@/lib/session";
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
          id: 1,
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
          id: 2,
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

describe("The session", () => {
  beforeEach(() => {
    setToken("stored-token");
  });

  it("shows the account the token was minted for", async () => {
    mockApi({
      "/api/v1/hello": () => Response.json({ message: "Hello from upland!" }),
      "/api/v1/auth/me": () =>
        Response.json({
          id: 1,
          username: "tess",
          email: "tess@example.com",
          identities: ["work"],
        }),
    });
    renderApp("/");

    expect(await screen.findByText("Hello from upland!")).toBeInTheDocument();
    expect(screen.getByText(/tess@example\.com/)).toBeInTheDocument();
  });

  it("brings an expired session back to the login screen", async () => {
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
          id: 1,
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
          id: 1,
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
