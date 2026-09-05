import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import React from "react";
import ReactDOM from "react-dom/client";
import { createBrowserRouter, RouterProvider } from "react-router-dom";
import { Layout } from "./Layout";
import "./index.css";

const queryClient = new QueryClient({
  defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false } },
});

// RFC 0136 §7 (performance pass) — every page used to be a top-level `import`, so the whole
// console (every page's own dependencies, `recharts` for `Dashboard` included) shipped as one
// eagerly-loaded bundle regardless of which single page a visit actually needed. React Router's
// per-route `lazy` (v6.4+) splits each page into its own chunk, fetched only when its route is
// visited — the same lazy-loading `GraphCanvas` already used on its own, just applied
// consistently at the route level instead of one component at a time.
const router = createBrowserRouter([
  {
    path: "/",
    element: <Layout />,
    children: [
      {
        index: true,
        lazy: () => import("./pages/Workspaces").then((m) => ({ Component: m.Workspaces })),
      },
      {
        path: "schedules",
        lazy: () => import("./pages/Schedules").then((m) => ({ Component: m.Schedules })),
      },
      {
        path: "runs/:runId",
        lazy: () => import("./pages/RunDetail").then((m) => ({ Component: m.RunDetail })),
      },
      {
        path: "w/:id",
        lazy: () =>
          import("./WorkspaceShell").then((m) => ({ Component: m.WorkspaceShell })),
        children: [
          {
            index: true,
            lazy: () => import("./pages/Dashboard").then((m) => ({ Component: m.Dashboard })),
          },
          {
            path: "graph",
            lazy: () => import("./pages/Graph").then((m) => ({ Component: m.Graph })),
          },
          { path: "run", lazy: () => import("./pages/Run").then((m) => ({ Component: m.Run })) },
          {
            path: "runs",
            lazy: () => import("./pages/Runs").then((m) => ({ Component: m.Runs })),
          },
          {
            path: "config",
            lazy: () => import("./pages/Config").then((m) => ({ Component: m.Config })),
          },
        ],
      },
    ],
  },
]);

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  </React.StrictMode>,
);
