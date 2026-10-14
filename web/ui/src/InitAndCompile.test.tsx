import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as client from "./api/client";
import { InitAndCompile } from "./InitAndCompile";

function renderIt() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <MemoryRouter initialEntries={["/w/shop"]}>
        <Routes>
          <Route path="/w/:id" element={<InitAndCompile workspace="shop" name="Shop" />} />
          <Route path="/runs/:runId" element={<p>run page</p>} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

afterEach(() => vi.restoreAllMocks());

describe("init & compile button", () => {
  it("asks first, then starts the run and opens it", async () => {
    const post = vi.spyOn(client, "apiPost").mockResolvedValue({ run_id: "r1" });
    vi.spyOn(window, "confirm").mockReturnValue(true);
    renderIt();
    fireEvent.click(screen.getByRole("button", { name: "Init & compile" }));
    await waitFor(() => expect(screen.getByText("run page")).toBeTruthy());
    expect(post).toHaveBeenCalledWith("/workspaces/shop/commands/init-and-compile");
  });

  it("does nothing when not confirmed", () => {
    const post = vi.spyOn(client, "apiPost").mockResolvedValue({ run_id: "r1" });
    vi.spyOn(window, "confirm").mockReturnValue(false);
    renderIt();
    fireEvent.click(screen.getByRole("button", { name: "Init & compile" }));
    expect(post).not.toHaveBeenCalled();
  });
});
