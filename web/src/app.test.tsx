import { screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { renderApp } from "@/test/harness";

describe("Home", () => {
  it("shows the greeting returned by the API", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValue(Response.json({ message: "Hello from upland!" }));
    vi.stubGlobal("fetch", fetchMock);

    renderApp();

    expect(await screen.findByText("Hello from upland!")).toBeInTheDocument();
    expect(fetchMock).toHaveBeenCalledWith("/api/hello");
  });
});

describe("API failures", () => {
  it("shows an error view when the greeting cannot be loaded", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(new Response(null, { status: 500 })),
    );

    renderApp();

    expect(await screen.findByText("Something went wrong")).toBeInTheDocument();
    expect(
      screen.getByText("Could not reach the upland API."),
    ).toBeInTheDocument();
  });
});

describe("Unknown routes", () => {
  it("shows a not-found view for any unknown path", async () => {
    renderApp("/does-not-exist");

    expect(await screen.findByText("Page not found")).toBeInTheDocument();
    expect(screen.getByRole("banner")).toHaveTextContent("Upland");
  });
});
