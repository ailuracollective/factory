import React, { useState } from "react";
import ReactDOM from "react-dom/client";
import { QueryClient, QueryClientProvider, useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  RouterProvider,
  createRouter,
  createRoute,
  createRootRoute,
  Outlet,
} from "@tanstack/react-router";

const GATEWAY = (import.meta as any).env?.VITE_GATEWAY_URL || "http://localhost:8787";

function JobsView() {
  const [token, setToken] = useState(localStorage.getItem("adminToken") || "");
  const qc = useQueryClient();
  const { data, isLoading, error } = useQuery({
    queryKey: ["jobs", token],
    queryFn: async () => {
      const res = await fetch(`${GATEWAY}/admin/jobs`, {
        headers: { "x-admin-token": token },
      });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      return res.json();
    },
    enabled: !!token,
  });
  const cancel = useMutation({
    mutationFn: async (id: string) => {
      await fetch(`${GATEWAY}/admin/jobs/${encodeURIComponent(id)}/cancel`, {
        method: "POST",
        headers: { "x-admin-token": token },
      });
    },
    onSuccess: () => qc.invalidateQueries({ queryKey: ["jobs", token] }),
  });

  return (
    <div>
      <label>
        Admin token:{" "}
        <input
          value={token}
          onChange={(e) => {
            setToken(e.target.value);
            localStorage.setItem("adminToken", e.target.value);
          }}
        />
      </label>
      {isLoading && <p>Loading...</p>}
      {error && <p style={{ color: "red" }}>{String(error)}</p>}
      <ul>
        {(data?.jobs || []).map((j: any) => (
          <li key={j.id}>
            {j.id} — {j.status} (attempts {j.attempts})
            {j.status !== "completed" && j.status !== "cancelled" && (
              <button onClick={() => cancel.mutate(j.id)}>Cancel</button>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}

const rootRoute = createRootRoute({
  component: () => (
    <div>
      <h1>Ailura Factory Dashboard</h1>
      <Outlet />
    </div>
  ),
});

const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: JobsView,
});

const routeTree = rootRoute.addChildren([indexRoute]);
const router = createRouter({ routeTree });
const queryClient = new QueryClient();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  </React.StrictMode>,
);
