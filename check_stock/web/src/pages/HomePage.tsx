import { Link } from "@tanstack/react-router";
import { can, useMe } from "../api/queries";
import { NAV_ITEMS } from "../components/AppLayout";

const DESCRIPTIONS: Record<string, string> = {
  "/singles": "Look up cards by set and collector number, with image and prices.",
  "/users": "Create accounts and choose what each person may do.",
  "/audit": "Who did what and when.",
};

export function HomePage() {
  const { data: me } = useMe();
  const sections = NAV_ITEMS.filter((i) => i.to !== "/" && (!i.permission || can(me, i.permission)));
  return (
    <section className="page">
      <h1>Hello, {me?.display_name}</h1>
      <p className="muted">
        The screens of the check_stock desktop app move here step by step. Until then the desktop app stays in use.
      </p>
      <div className="tiles">
        {sections.map((s) => (
          <Link key={s.to} to={s.to} className="tile">
            <span className="tile__title">{s.label}</span>
            <span className="tile__text">{DESCRIPTIONS[s.to]}</span>
          </Link>
        ))}
        <Link to="/account" className="tile">
          <span className="tile__title">Your account</span>
          <span className="tile__text">Your permissions and password.</span>
        </Link>
      </div>
    </section>
  );
}
