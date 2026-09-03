import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import React from "react";
import ReactDOM from "react-dom/client";
import { createBrowserRouter, RouterProvider } from "react-router-dom";
import { Layout } from "./Layout";
import { Config } from "./pages/Config";
import { Dashboard } from "./pages/Dashboard";
import { Graph } from "./pages/Graph";
import { Run } from "./pages/Run";
import { RunDetail } from "./pages/RunDetail";
import { Runs } from "./pages/Runs";
import { Schedules } from "./pages/Schedules";
import { Workspaces } from "./pages/Workspaces";
import { WorkspaceShell } from "./WorkspaceShell";
import "./index.css";

const queryClient = new QueryClient({
  defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false } },
});

const router = createBrowserRouter([
  {
    path: "/",
    element: <Layout />,
    children: [
      { index: true, element: <Workspaces /> },
      { path: "schedules", element: <Schedules /> },
      { path: "runs/:runId", element: <RunDetail /> },
      {
        path: "w/:id",
        element: <WorkspaceShell />,
        children: [
          { index: true, element: <Dashboard /> },
          { path: "graph", element: <Graph /> },
          { path: "run", element: <Run /> },
          { path: "runs", element: <Runs /> },
          { path: "config", element: <Config /> },
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
