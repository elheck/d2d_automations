import { Link } from "@tanstack/react-router";
import { useMe } from "../api/queries";
import { visibleGroups } from "../navigation";

export function HomePage() {
  const { data: me } = useMe();
  return (
    <section className="page">
      <h1>Hello, {me?.display_name}</h1>
      <p className="muted">
        The screens of the check_stock desktop app move here step by step. Until then the desktop app stays in use.
      </p>
      {visibleGroups(me).map((group) => (
        <section key={group.label} aria-label={group.label}>
          <h2>{group.label}</h2>
          <div className="tiles">
            {group.items.map((item) => (
              <Link key={item.to} to={item.to} className="tile">
                <span className="tile__title">{item.label}</span>
                <span className="tile__text">{item.description}</span>
              </Link>
            ))}
          </div>
        </section>
      ))}
      <h2>You</h2>
      <div className="tiles">
        <Link to="/account" className="tile">
          <span className="tile__title">Your account</span>
          <span className="tile__text">Your permissions and password.</span>
        </Link>
      </div>
    </section>
  );
}
