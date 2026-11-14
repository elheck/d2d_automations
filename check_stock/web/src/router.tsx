// Pages and their addresses. Everything except /login sits inside the app
// layout, which sends visitors without a session to the login page.

import { createRootRoute, createRoute, createRouter, Outlet, type RouterHistory } from "@tanstack/react-router";
import { AppLayout } from "./components/AppLayout";
import { RequirePermission } from "./components/RequirePermission";
import { AccountPage } from "./pages/AccountPage";
import { AuditPage } from "./pages/AuditPage";
import { HomePage } from "./pages/HomePage";
import { LoginPage } from "./pages/LoginPage";
import { NotFoundPage } from "./pages/NotFoundPage";
import { UsersPage } from "./pages/UsersPage";

/**
 * Where to go after logging in: only paths inside this app (no `//host`,
 * no other scheme), so the login page cannot be used to send people away.
 */
export function safeRedirect(target: unknown): string | undefined {
  if (typeof target !== "string") return undefined;
  if (!target.startsWith("/") || target.startsWith("//") || target.startsWith("/\\")) return undefined;
  if (target === "/login" || target.startsWith("/login?")) return undefined;
  return target;
}

const rootRoute = createRootRoute({
  component: Outlet,
  notFoundComponent: NotFoundPage,
});

const loginRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/login",
  validateSearch: (search: Record<string, unknown>): { redirect?: string } => {
    const redirect = safeRedirect(search.redirect);
    return redirect ? { redirect } : {};
  },
  component: LoginPage,
});

const appRoute = createRoute({
  getParentRoute: () => rootRoute,
  id: "app",
  component: AppLayout,
});

const homeRoute = createRoute({
  getParentRoute: () => appRoute,
  path: "/",
  component: HomePage,
});

const usersRoute = createRoute({
  getParentRoute: () => appRoute,
  path: "/users",
  component: () => (
    <RequirePermission permission="users.manage">
      <UsersPage />
    </RequirePermission>
  ),
});

const auditRoute = createRoute({
  getParentRoute: () => appRoute,
  path: "/audit",
  component: () => (
    <RequirePermission permission="audit.view">
      <AuditPage />
    </RequirePermission>
  ),
});

const accountRoute = createRoute({
  getParentRoute: () => appRoute,
  path: "/account",
  component: AccountPage,
});

const routeTree = rootRoute.addChildren([
  loginRoute,
  appRoute.addChildren([homeRoute, usersRoute, auditRoute, accountRoute]),
]);

export function createAppRouter(history?: RouterHistory) {
  return createRouter({ routeTree, history, defaultPreload: false });
}

export type AppRouter = ReturnType<typeof createAppRouter>;

declare module "@tanstack/react-router" {
  interface Register {
    router: AppRouter;
  }
}
