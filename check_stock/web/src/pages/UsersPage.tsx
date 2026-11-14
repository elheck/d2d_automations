// User administration: accounts, their permissions (with the server's
// descriptions), passwords, lockouts. The server enforces every rule; the
// page only avoids offering what it would refuse anyway.

import { useState, type FormEvent } from "react";
import {
  useCreateUser,
  useDeleteUser,
  useMe,
  usePermissions,
  useSetPassword,
  useUpdateUser,
  useUsers,
} from "../api/queries";
import type { PermissionInfo, UpdateUserRequest, UserView } from "../api/types";
import { ErrorMessage } from "../components/ErrorMessage";
import { Modal } from "../components/Modal";
import { PermissionPicker } from "../components/PermissionPicker";
import { MIN_PASSWORD_CHARS } from "./AccountPage";

type Dialog =
  | { kind: "create" }
  | { kind: "edit"; user: UserView }
  | { kind: "password"; user: UserView }
  | { kind: "delete"; user: UserView };

export function formatTime(iso: string | null): string {
  return iso ? new Date(iso).toLocaleString() : "—";
}

function status(user: UserView): string {
  if (user.disabled) return "Disabled";
  if (user.locked_until) return `Locked until ${formatTime(user.locked_until)}`;
  return "Active";
}

export function UsersPage() {
  const { data: me } = useMe();
  const users = useUsers();
  const permissions = usePermissions();
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const catalog = permissions.data ?? [];
  const close = () => setDialog(null);

  return (
    <section className="page">
      <div className="page__header">
        <h1>Users</h1>
        <button
          type="button"
          className="button button--primary"
          onClick={() => setDialog({ kind: "create" })}
          disabled={!permissions.data}
        >
          New user
        </button>
      </div>
      <ErrorMessage error={users.error ?? permissions.error} />
      {users.isPending ? (
        <p className="loading">Loading…</p>
      ) : (
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                <th>Login</th>
                <th>Name</th>
                <th>Status</th>
                <th>Permissions</th>
                <th>Last login</th>
                <th aria-label="Actions" />
              </tr>
            </thead>
            <tbody>
              {(users.data ?? []).map((user) => {
                const self = user.username === me?.username;
                return (
                  <tr key={user.id}>
                    <td>
                      {user.username}
                      {self && <span className="badge">you</span>}
                    </td>
                    <td>{user.display_name}</td>
                    <td>{status(user)}</td>
                    <td>
                      {catalog.length > 0 && user.permissions.length === catalog.length
                        ? "All"
                        : `${user.permissions.length} of ${catalog.length}`}
                    </td>
                    <td>{formatTime(user.last_login_at)}</td>
                    <td className="table__actions">
                      <button
                        type="button"
                        className="button button--small"
                        disabled={!permissions.data}
                        onClick={() => setDialog({ kind: "edit", user })}
                      >
                        Edit
                      </button>
                      <button
                        type="button"
                        className="button button--small"
                        onClick={() => setDialog({ kind: "password", user })}
                      >
                        Password
                      </button>
                      <button
                        type="button"
                        className="button button--small button--danger"
                        disabled={self}
                        title={self ? "You cannot delete your own account" : undefined}
                        onClick={() => setDialog({ kind: "delete", user })}
                      >
                        Delete
                      </button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}

      {dialog?.kind === "create" && <CreateUserDialog catalog={catalog} onClose={close} />}
      {dialog?.kind === "edit" && (
        <EditUserDialog
          user={dialog.user}
          catalog={catalog}
          self={dialog.user.username === me?.username}
          onClose={close}
        />
      )}
      {dialog?.kind === "password" && <PasswordDialog user={dialog.user} onClose={close} />}
      {dialog?.kind === "delete" && <DeleteDialog user={dialog.user} onClose={close} />}
    </section>
  );
}

function passwordProblem(password: string, repeat: string): string | null {
  if ([...password].length < MIN_PASSWORD_CHARS) return `The password needs at least ${MIN_PASSWORD_CHARS} characters.`;
  if (password !== repeat) return "The two passwords differ.";
  return null;
}

function CreateUserDialog({ catalog, onClose }: { catalog: PermissionInfo[]; onClose: () => void }) {
  const create = useCreateUser();
  const [username, setUsername] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [password, setPassword] = useState("");
  const [repeat, setRepeat] = useState("");
  const [granted, setGranted] = useState<string[]>([]);
  const [problem, setProblem] = useState<string | null>(null);

  const submit = (e: FormEvent) => {
    e.preventDefault();
    const issue = passwordProblem(password, repeat);
    setProblem(issue);
    if (issue) return;
    create.mutate({ username, display_name: displayName, password, permissions: granted }, { onSuccess: onClose });
  };

  return (
    <Modal title="New user" onClose={onClose}>
      <form className="form" onSubmit={submit}>
        <label className="field">
          <span>Login name (letters a-z, digits, . _ -)</span>
          <input
            autoComplete="off"
            autoCapitalize="none"
            spellCheck={false}
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            required
          />
        </label>
        <label className="field">
          <span>Display name</span>
          <input autoComplete="off" value={displayName} onChange={(e) => setDisplayName(e.target.value)} required />
        </label>
        <label className="field">
          <span>Password (at least {MIN_PASSWORD_CHARS} characters)</span>
          <input
            type="password"
            autoComplete="new-password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            required
          />
        </label>
        <label className="field">
          <span>Repeat password</span>
          <input
            type="password"
            autoComplete="new-password"
            value={repeat}
            onChange={(e) => setRepeat(e.target.value)}
            required
          />
        </label>
        <h3>Permissions</h3>
        <PermissionPicker catalog={catalog} value={granted} onChange={setGranted} />
        {problem && (
          <p className="error" role="alert">
            {problem}
          </p>
        )}
        <ErrorMessage error={create.error} />
        <div className="form__actions">
          <button type="button" className="button" onClick={onClose}>
            Cancel
          </button>
          <button type="submit" className="button button--primary" disabled={create.isPending}>
            Create user
          </button>
        </div>
      </form>
    </Modal>
  );
}

/** Only what changed, so an unchanged field is never overwritten. */
export function userChanges(
  user: UserView,
  edited: { displayName: string; disabled: boolean; permissions: string[] },
): UpdateUserRequest {
  const changes: UpdateUserRequest = {};
  if (edited.displayName.trim() !== user.display_name) changes.display_name = edited.displayName;
  if (edited.disabled !== user.disabled) changes.disabled = edited.disabled;
  const before = [...user.permissions].sort().join();
  const after = [...edited.permissions].sort().join();
  if (before !== after) changes.permissions = edited.permissions;
  return changes;
}

function EditUserDialog({
  user,
  catalog,
  self,
  onClose,
}: {
  user: UserView;
  catalog: PermissionInfo[];
  self: boolean;
  onClose: () => void;
}) {
  const update = useUpdateUser();
  const [displayName, setDisplayName] = useState(user.display_name);
  const [disabled, setDisabled] = useState(user.disabled);
  const [granted, setGranted] = useState<string[]>(user.permissions);
  const changes = userChanges(user, { displayName, disabled, permissions: granted });
  const unchanged = Object.keys(changes).length === 0;

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (unchanged) return onClose();
    update.mutate({ id: user.id, changes }, { onSuccess: onClose });
  };

  return (
    <Modal title={`Edit ${user.username}`} onClose={onClose}>
      <form className="form" onSubmit={submit}>
        <label className="field">
          <span>Display name</span>
          <input autoComplete="off" value={displayName} onChange={(e) => setDisplayName(e.target.value)} required />
        </label>
        <label className="checkbox">
          <input type="checkbox" checked={disabled} disabled={self} onChange={(e) => setDisabled(e.target.checked)} />
          <span>
            Account disabled (cannot log in; open sessions end)
            {self && <span className="muted"> — you cannot disable your own account</span>}
          </span>
        </label>
        {user.locked_until && (
          <p>
            Locked after failed logins until {formatTime(user.locked_until)}.{" "}
            <button
              type="button"
              className="button button--small"
              disabled={update.isPending}
              onClick={() => update.mutate({ id: user.id, changes: { unlock: true } }, { onSuccess: onClose })}
            >
              Unlock now
            </button>
          </p>
        )}
        <h3>Permissions</h3>
        <PermissionPicker catalog={catalog} value={granted} onChange={setGranted} />
        <ErrorMessage error={update.error} />
        <div className="form__actions">
          <button type="button" className="button" onClick={onClose}>
            Cancel
          </button>
          <button type="submit" className="button button--primary" disabled={update.isPending || unchanged}>
            Save
          </button>
        </div>
      </form>
    </Modal>
  );
}

function PasswordDialog({ user, onClose }: { user: UserView; onClose: () => void }) {
  const setPasswordMutation = useSetPassword();
  const [password, setPassword] = useState("");
  const [repeat, setRepeat] = useState("");
  const [problem, setProblem] = useState<string | null>(null);

  const submit = (e: FormEvent) => {
    e.preventDefault();
    const issue = passwordProblem(password, repeat);
    setProblem(issue);
    if (issue) return;
    setPasswordMutation.mutate({ id: user.id, password }, { onSuccess: onClose });
  };

  return (
    <Modal title={`New password for ${user.username}`} onClose={onClose}>
      <form className="form" onSubmit={submit}>
        <p className="muted">Also unlocks the account and logs it out everywhere.</p>
        <label className="field">
          <span>New password (at least {MIN_PASSWORD_CHARS} characters)</span>
          <input
            type="password"
            autoComplete="new-password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            required
          />
        </label>
        <label className="field">
          <span>Repeat password</span>
          <input
            type="password"
            autoComplete="new-password"
            value={repeat}
            onChange={(e) => setRepeat(e.target.value)}
            required
          />
        </label>
        {problem && (
          <p className="error" role="alert">
            {problem}
          </p>
        )}
        <ErrorMessage error={setPasswordMutation.error} />
        <div className="form__actions">
          <button type="button" className="button" onClick={onClose}>
            Cancel
          </button>
          <button type="submit" className="button button--primary" disabled={setPasswordMutation.isPending}>
            Set password
          </button>
        </div>
      </form>
    </Modal>
  );
}

function DeleteDialog({ user, onClose }: { user: UserView; onClose: () => void }) {
  const remove = useDeleteUser();
  return (
    <Modal title={`Delete ${user.username}?`} onClose={onClose}>
      <p>
        {user.display_name} can no longer log in. Their entries in the audit log stay. This cannot be undone; to only
        block the account, edit it and disable it instead.
      </p>
      <ErrorMessage error={remove.error} />
      <div className="form__actions">
        <button type="button" className="button" onClick={onClose}>
          Cancel
        </button>
        <button
          type="button"
          className="button button--danger"
          disabled={remove.isPending}
          onClick={() => remove.mutate(user.id, { onSuccess: onClose })}
        >
          Delete user
        </button>
      </div>
    </Modal>
  );
}
