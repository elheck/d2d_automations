// Choosing a user's permissions, grouped as the server defines them, each
// with its description and what it needs. Ticking a permission also ticks
// what it requires; unticking one also unticks everything that depends on it,
// so the set always satisfies the server's rules.

import type { PermissionInfo } from "../api/types";

/** Sensitive enough to warrant a visible warning. */
const DANGEROUS = new Set(["users.manage"]);

/** `key` plus everything it needs, transitively. */
export function withRequirements(catalog: PermissionInfo[], key: string): Set<string> {
  const byKey = new Map(catalog.map((p) => [p.key, p]));
  const result = new Set<string>();
  const visit = (k: string) => {
    if (result.has(k)) return;
    result.add(k);
    byKey.get(k)?.requires.forEach(visit);
  };
  visit(key);
  return result;
}

/** `key` plus everything that needs it, transitively. */
export function withDependents(catalog: PermissionInfo[], key: string): Set<string> {
  const result = new Set<string>([key]);
  let grew = true;
  while (grew) {
    grew = false;
    for (const p of catalog) {
      if (!result.has(p.key) && p.requires.some((r) => result.has(r))) {
        result.add(p.key);
        grew = true;
      }
    }
  }
  return result;
}

/** The next selection after (un)ticking `key`, in catalog order. */
export function toggle(catalog: PermissionInfo[], selected: string[], key: string, on: boolean): string[] {
  const next = new Set(selected);
  if (on) withRequirements(catalog, key).forEach((k) => next.add(k));
  else withDependents(catalog, key).forEach((k) => next.delete(k));
  return catalog.map((p) => p.key).filter((k) => next.has(k));
}

interface Props {
  catalog: PermissionInfo[];
  value: string[];
  onChange: (next: string[]) => void;
  disabled?: boolean;
}

export function PermissionPicker({ catalog, value, onChange, disabled = false }: Props) {
  const labels = new Map(catalog.map((p) => [p.key, p.label]));
  const groups: { label: string; items: PermissionInfo[] }[] = [];
  for (const permission of catalog) {
    const last = groups[groups.length - 1];
    if (last && last.label === permission.group_label) last.items.push(permission);
    else groups.push({ label: permission.group_label, items: [permission] });
  }

  return (
    <div className="permission-picker">
      <div className="permission-picker__bulk">
        <button
          type="button"
          className="button button--small"
          disabled={disabled}
          onClick={() => onChange(catalog.map((p) => p.key))}
        >
          All
        </button>
        <button type="button" className="button button--small" disabled={disabled} onClick={() => onChange([])}>
          None
        </button>
        <span className="muted">
          {value.length} of {catalog.length} granted
        </span>
      </div>
      {groups.map((group) => (
        <fieldset key={group.label} className="permission-group" disabled={disabled}>
          <legend>{group.label}</legend>
          {group.items.map((p) => {
            const id = `permission-${p.key}`;
            const checked = value.includes(p.key);
            return (
              <div key={p.key} className={DANGEROUS.has(p.key) ? "permission permission--danger" : "permission"}>
                <input
                  id={id}
                  type="checkbox"
                  checked={checked}
                  onChange={(e) => onChange(toggle(catalog, value, p.key, e.target.checked))}
                  aria-describedby={`${id}-description`}
                />
                <div className="permission__text">
                  <label htmlFor={id} className="permission__label">
                    {p.label}
                  </label>
                  <p id={`${id}-description`} className="permission__description">
                    {p.description}
                    {p.requires.length > 0 && (
                      <span className="permission__requires">
                        {" "}
                        Needs: {p.requires.map((r) => labels.get(r) ?? r).join(", ")}.
                      </span>
                    )}
                  </p>
                </div>
              </div>
            );
          })}
        </fieldset>
      ))}
    </div>
  );
}
