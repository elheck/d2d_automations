import { useState, type FormEvent } from "react";
import { useChangePassword, useMe, usePermissions } from "../api/queries";
import { ErrorMessage } from "../components/ErrorMessage";

export const MIN_PASSWORD_CHARS = 12;

export function AccountPage() {
  const { data: me } = useMe();
  const { data: catalog = [] } = usePermissions();
  const change = useChangePassword();
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [repeat, setRepeat] = useState("");
  const [localError, setLocalError] = useState<string | null>(null);
  const [done, setDone] = useState(false);

  const granted = catalog.filter((p) => me?.permissions.includes(p.key));

  const submit = (e: FormEvent) => {
    e.preventDefault();
    setDone(false);
    if ([...next].length < MIN_PASSWORD_CHARS) {
      setLocalError(`The new password needs at least ${MIN_PASSWORD_CHARS} characters.`);
      return;
    }
    if (next !== repeat) {
      setLocalError("The two new passwords differ.");
      return;
    }
    setLocalError(null);
    change.mutate(
      { current_password: current, new_password: next },
      {
        onSuccess: () => {
          setCurrent("");
          setNext("");
          setRepeat("");
          setDone(true);
        },
      },
    );
  };

  return (
    <section className="page">
      <h1>Your account</h1>
      <p>
        Logged in as <strong>{me?.username}</strong> ({me?.display_name}).
      </p>

      <h2>Your permissions</h2>
      {granted.length === 0 ? (
        <p className="muted">No permissions yet. Ask an administrator.</p>
      ) : (
        <ul className="permission-list">
          {granted.map((p) => (
            <li key={p.key}>
              <strong>{p.label}</strong> <span className="muted">— {p.description}</span>
            </li>
          ))}
        </ul>
      )}

      <h2>Change password</h2>
      <form className="form" onSubmit={submit}>
        <input type="text" name="username" autoComplete="username" value={me?.username ?? ""} readOnly hidden />
        <label className="field">
          <span>Current password</span>
          <input
            type="password"
            autoComplete="current-password"
            value={current}
            onChange={(e) => setCurrent(e.target.value)}
            required
          />
        </label>
        <label className="field">
          <span>New password (at least {MIN_PASSWORD_CHARS} characters)</span>
          <input
            type="password"
            autoComplete="new-password"
            value={next}
            onChange={(e) => setNext(e.target.value)}
            required
          />
        </label>
        <label className="field">
          <span>Repeat new password</span>
          <input
            type="password"
            autoComplete="new-password"
            value={repeat}
            onChange={(e) => setRepeat(e.target.value)}
            required
          />
        </label>
        {localError && (
          <p className="error" role="alert">
            {localError}
          </p>
        )}
        <ErrorMessage error={change.error} />
        {done && (
          <p className="success" role="status">
            Password changed. Your other sessions were logged out.
          </p>
        )}
        <button type="submit" className="button button--primary" disabled={change.isPending}>
          Change password
        </button>
      </form>
    </section>
  );
}
