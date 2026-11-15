// Frame of every page after login: navigation (only what the user may open),
// the user's name, account and logout.

import { Link, Outlet, useLocation, useNavigate } from "@tanstack/react-router";
import { useEffect, useRef } from "react";
import { can, useLogout, useMe } from "../api/queries";

export interface NavItem {
  to: "/" | "/singles" | "/buy-helper" | "/precon-ev" | "/users" | "/audit";
  label: string;
  permission?: string;
}

export const NAV_ITEMS: NavItem[] = [
  { to: "/", label: "Home" },
  { to: "/singles", label: "Singles", permission: "listing.use" },
  { to: "/buy-helper", label: "Buy helper", permission: "buy_helper.use" },
  { to: "/precon-ev", label: "Precon EV", permission: "precon_ev.use" },
  { to: "/users", label: "Users", permission: "users.manage" },
  { to: "/audit", label: "Audit log", permission: "audit.view" },
];

export function AppLayout() {
  const { data: me, isPending, error } = useMe();
  const logout = useLogout();
  const navigate = useNavigate();
  const location = useLocation();

  // Where to come back to after logging in: the last page seen with a
  // session (taken before the redirect, so the redirect cannot change it).
  const returnTo = useRef(location.href);
  useEffect(() => {
    if (me) returnTo.current = location.href;
  }, [me, location.href]);
  // After an explicit logout the next person starts fresh, not on the
  // previous user's page; only an expired session keeps the way back.
  const loggedOut = useRef(false);
  const needsLogin = !isPending && !error && !me;

  // Navigate once when the session is gone (not on every render: a
  // navigation per render would loop while this layout is still mounted).
  useEffect(() => {
    if (!needsLogin) return;
    const back = loggedOut.current || returnTo.current === "/" ? undefined : returnTo.current;
    void navigate({ to: "/login", search: back ? { redirect: back } : {}, replace: true });
  }, [needsLogin, navigate]);

  if (isPending) return <p className="loading">Loading…</p>;
  if (error) {
    return (
      <p className="error" role="alert">
        The server cannot be reached. Try again in a moment.
      </p>
    );
  }
  if (!me) return <p className="loading">Opening the login…</p>;

  return (
    <div className="app">
      <header className="app-header">
        <Link to="/" className="app-header__brand">
          check_stock
        </Link>
        <nav aria-label="Main" className="app-nav">
          {NAV_ITEMS.filter((item) => !item.permission || can(me, item.permission)).map((item) => (
            <Link
              key={item.to}
              to={item.to}
              className="app-nav__link"
              activeOptions={{ exact: item.to === "/" }}
              activeProps={{ className: "app-nav__link app-nav__link--active" }}
            >
              {item.label}
            </Link>
          ))}
        </nav>
        <div className="app-header__user">
          <Link
            to="/account"
            className="app-nav__link"
            activeProps={{ className: "app-nav__link app-nav__link--active" }}
          >
            {me.display_name}
          </Link>
          <button
            type="button"
            className="button button--ghost"
            disabled={logout.isPending}
            onClick={() => {
              loggedOut.current = true;
              logout.mutate();
            }}
          >
            Log out
          </button>
        </div>
      </header>
      <main className="app-main">
        <Outlet />
      </main>
    </div>
  );
}
