import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import React from "react";
import ReactDOM from "react-dom/client";
import { createBrowserRouter, RouterProvider } from "react-router-dom";
import { Layout } from "./Layout";
import { Config } from "./pages/Config";
import { Dashboard } from "./pages/Dashboard";
import { Run } from "./pages/Run";
import { RunDetail } from "./pages/RunDetail";
import { Runs } from "./pages/Runs";
import { Workspaces } from "./pages/Workspaces";
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
      { path: "w/:id", element: <Dashboard /> },
      { path: "w/:id/config", element: <Config /> },
      { path: "w/:id/run", element: <Run /> },
      { path: "w/:id/runs", element: <Runs /> },
      { path: "runs/:runId", element: <RunDetail /> },
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
