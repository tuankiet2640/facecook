import { useEffect } from "react";
import { Outlet, Navigate } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import { usersApi } from "@/lib/api";
import { useAuthStore } from "@/stores/auth";
import { qk } from "@/lib/queryKeys";
import { Sidebar } from "./Sidebar";

/**
 * AppShell wraps all authenticated routes.
 * - Redirects to /auth if the user isn't logged in.
 * - Fetches /users/me once and stores the result in Zustand.
 * - Renders the left sidebar + the current page via <Outlet />.
 */
export function AppShell() {
  const token = useAuthStore((s) => s.token);
  const setUser = useAuthStore((s) => s.setUser);

  const { data: me } = useQuery({
    queryKey: qk.me(),
    queryFn: usersApi.me,
    enabled: !!token,
    staleTime: 60_000,
    retry: false,
  });

  useEffect(() => {
    if (me) setUser(me);
  }, [me, setUser]);

  if (!token) return <Navigate to="/auth" replace />;

  return (
    <div className="h-full flex overflow-hidden">
      <Sidebar />
      <main className="flex-1 overflow-y-auto min-w-0">
        <Outlet />
      </main>
    </div>
  );
}
