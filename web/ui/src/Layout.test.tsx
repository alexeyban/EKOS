import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RouterProvider, createMemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Layout } from "./Layout";
import * as client from "./api/client";

const { ApiError } = client;

function renderLayout() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const router = createMemoryRouter(
    [{ path: "/", element: <Layout />, children: [{ index: true, element: <p>workspace list</p> }] }],
    { initialEntries: ["/"] },
  );
  return render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("Layout sign-out", () => {
  it("returns to the sign-in screen after logout 401s the me query", async () => {
    let signedIn = true;
    vi.spyOn(client, "api").mockImplementation((path: string) => {
      if (path === "/auth/me") {
        return signedIn
          ? Promise.resolve({ mode: "token", email: null, role: "read" })
          : Promise.reject(new ApiError(401, "not authenticated", { mode: "token" }));
      }
      throw new Error(`unexpected path ${path}`);
    });
    const logoutSpy = vi.spyOn(client, "logout").mockImplementation(async () => {
      signedIn = false;
      return {};
    });

    renderLayout();

    // Authenticated view is shown first.
    expect(await screen.findByText("workspace list")).toBeInTheDocument();
    const button = screen.getByRole("button", { name: "sign out" });

    await userEvent.click(button);

    // The stale `me.data` must not keep the console looking signed in: a logout 401s the `me`
    // refetch, which must drop us back to the token entry form.
    expect(await screen.findByLabelText("console token")).toBeInTheDocument();
    expect(logoutSpy).toHaveBeenCalled();
    expect(screen.queryByText("workspace list")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "sign out" })).not.toBeInTheDocument();
  });
});
