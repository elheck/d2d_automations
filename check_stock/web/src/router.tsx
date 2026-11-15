// Pages and their addresses. Everything except /login sits inside the app
// layout, which sends visitors without a session to the login page.

import { createRootRoute, createRoute, createRouter, Outlet, type RouterHistory } from "@tanstack/react-router";
import { AppLayout } from "./components/AppLayout";
import { RequirePermission } from "./components/RequirePermission";
import { AccountPage } from "./pages/AccountPage";
import { AuditPage } from "./pages/AuditPage";
import { BuyHelperPage } from "./pages/BuyHelperPage";
import { HomePage } from "./pages/HomePage";
import { LoginPage } from "./pages/LoginPage";
import { MispricingPage } from "./pages/MispricingPage";
import { MoversPage } from "./pages/MoversPage";
import { NotFoundPage } from "./pages/NotFoundPage";
import { PreconEvPage } from "./pages/PreconEvPage";
import { PreconProductPage } from "./pages/PreconProductPage";
import { RestockPage } from "./pages/RestockPage";
import { StockAnalysisPage } from "./pages/StockAnalysisPage";
import { SinglesPage } from "./pages/SinglesPage";
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

const singlesRoute = createRoute({
  getParentRoute: () => appRoute,
  path: "/singles",
  component: () => (
    <RequirePermission permission="listing.use">
      <SinglesPage />
    </RequirePermission>
  ),
});

const buyHelperRoute = createRoute({
  getParentRoute: () => appRoute,
  path: "/buy-helper",
  component: () => (
    <RequirePermission permission="buy_helper.use">
      <BuyHelperPage />
    </RequirePermission>
  ),
});

const preconRoute = createRoute({
  getParentRoute: () => appRoute,
  path: "/precon-ev",
  component: () => (
    <RequirePermission permission="precon_ev.use">
      <PreconEvPage />
    </RequirePermission>
  ),
});

const preconProductRoute = createRoute({
  getParentRoute: () => appRoute,
  path: "/precon-ev/$id",
  component: () => (
    <RequirePermission permission="precon_ev.use">
      <PreconProductPage />
    </RequirePermission>
  ),
});

const restockRoute = createRoute({
  getParentRoute: () => appRoute,
  path: "/restock",
  component: () => (
    <RequirePermission permission="analysis.view">
      <RestockPage />
    </RequirePermission>
  ),
});

const moversRoute = createRoute({
  getParentRoute: () => appRoute,
  path: "/movers",
  component: () => (
    <RequirePermission permission="analysis.view">
      <MoversPage />
    </RequirePermission>
  ),
});

const mispricingRoute = createRoute({
  getParentRoute: () => appRoute,
  path: "/mispricing",
  component: () => (
    <RequirePermission permission="mispricing.view">
      <MispricingPage />
    </RequirePermission>
  ),
});

const stockAnalysisRoute = createRoute({
  getParentRoute: () => appRoute,
  path: "/stock-analysis",
  component: () => (
    <RequirePermission permission="analysis.view">
      <StockAnalysisPage />
    </RequirePermission>
  ),
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
  appRoute.addChildren([
    homeRoute,
    singlesRoute,
    buyHelperRoute,
    preconRoute,
    preconProductRoute,
    restockRoute,
    moversRoute,
    mispricingRoute,
    stockAnalysisRoute,
    usersRoute,
    auditRoute,
    accountRoute,
  ]),
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
