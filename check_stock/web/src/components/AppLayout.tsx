// Frame of every page after login: navigation (only what the user may open),
// the user's name, account and logout.

import { Link, Outlet, useLocation, useNavigate } from "@tanstack/react-router";
import { useEffect, useRef } from "react";
import { useLogout, useMe } from "../api/queries";
import { groupIsActive, visibleGroups, type NavGroup } from "../navigation";

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
          <Link
            to="/"
            className="app-nav__link"
            activeOptions={{ exact: true }}
            activeProps={{ className: "app-nav__link app-nav__link--active" }}
          >
            Home
          </Link>
          {visibleGroups(me).map((group) => (
            <NavMenu key={group.label} group={group} path={location.pathname} />
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

/**
 * A group of the navigation as a drop-down (`<details>`): closes when one of
 * its links is followed, on a click elsewhere and on Escape.
 */
function NavMenu({ group, path }: { group: NavGroup; path: string }) {
  const menu = useRef<HTMLDetailsElement>(null);

  useEffect(() => {
    const onClick = (event: MouseEvent) => {
      const element = menu.current;
      if (element?.open && !element.contains(event.target as Node)) element.open = false;
    };
    const onKey = (event: KeyboardEvent) => {
      const element = menu.current;
      if (event.key === "Escape" && element?.open) {
        element.open = false;
        element.querySelector("summary")?.focus();
      }
    };
    document.addEventListener("click", onClick);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("click", onClick);
      document.removeEventListener("keydown", onKey);
    };
  }, []);

  const active = groupIsActive(group, path);
  return (
    <details className="nav-menu" ref={menu}>
      <summary className={active ? "app-nav__link app-nav__link--active" : "app-nav__link"}>{group.label}</summary>
      <div className="nav-menu__panel">
        {group.items.map((item) => (
          <Link
            key={item.to}
            to={item.to}
            className="nav-menu__link"
            activeProps={{ className: "nav-menu__link nav-menu__link--active" }}
            onClick={() => {
              if (menu.current) menu.current.open = false;
            }}
          >
            {item.label}
          </Link>
        ))}
      </div>
    </details>
  );
}
