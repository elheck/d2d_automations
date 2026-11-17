// Form building blocks of the rule panels (Bulk lots, Repricing): titled
// groups, labelled fields, toggle chips, key-figure tiles and a number field
// that accepts a comma or a point.

import { useState, type ReactNode } from "react";
import { numberText, parseNumber, type NumberRules } from "../pages/repricing";

/** A titled group of rules. */
export function RuleGroup({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="rule-group" role="group" aria-label={title}>
      <h3 className="rule-group__title">{title}</h3>
      {children}
    </div>
  );
}

/** A label above its control, with an optional hint below. */
export function Field({
  label,
  htmlFor,
  hint,
  children,
}: {
  label: string;
  htmlFor?: string;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <div className="rule-field">
      {htmlFor ? (
        <label className="rule-field__label" htmlFor={htmlFor}>
          {label}
        </label>
      ) : (
        <span className="rule-field__label">{label}</span>
      )}
      {children}
      {hint && <span className="rule-field__hint">{hint}</span>}
    </div>
  );
}

/** A toggle chip (pressed = included). */
export function Chip({
  on,
  onClick,
  children,
  title,
  disabled,
}: {
  on: boolean;
  onClick: () => void;
  children: ReactNode;
  title?: string;
  disabled?: boolean;
}) {
  return (
    <button type="button" className="chip" aria-pressed={on} onClick={onClick} title={title} disabled={disabled}>
      {children}
    </button>
  );
}

/** A key figure: small title, big value, one line below. */
export function Stat({
  label,
  value,
  sub,
  tone,
}: {
  label: string;
  value: string;
  sub: ReactNode;
  tone?: "good" | "warning" | "danger";
}) {
  return (
    <div className={tone ? `stat stat--${tone}` : "stat"}>
      <span className="stat__label">{label}</span>
      <span className="stat__value">{value}</span>
      <span className="stat__sub">{sub}</span>
    </div>
  );
}

/**
 * A number typed as text (comma or point), with a unit after it. Only valid
 * values reach `onChange`; invalid text stays in the field, marked, until it
 * is corrected. An optional field may be left empty (`null`).
 */
export function NumField({
  id,
  value,
  onChange,
  rules,
  unit,
  label,
  disabled,
}: {
  id: string;
  value: number | null;
  onChange: (value: number | null) => void;
  rules: NumberRules;
  unit?: string;
  /** For screen readers when no visible label points at the field. */
  label?: string;
  disabled?: boolean;
}) {
  const [text, setText] = useState(numberText(value));
  const [shown, setShown] = useState(value);
  // A new value from outside (another rule picked, a file opened) replaces
  // the text; the user's own valid typing already matches it.
  if (value !== shown) {
    setShown(value);
    if (parseNumber(text, rules) !== value) setText(numberText(value));
  }
  const invalid = parseNumber(text, rules) === undefined;
  return (
    <span className={invalid ? "unit-input unit-input--invalid" : "unit-input"}>
      <input
        id={id}
        inputMode={rules.integer ? "numeric" : "decimal"}
        value={text}
        aria-label={label}
        aria-invalid={invalid || undefined}
        disabled={disabled}
        onChange={(e) => {
          setText(e.target.value);
          const parsed = parseNumber(e.target.value, rules);
          if (parsed !== undefined) {
            setShown(parsed);
            onChange(parsed);
          }
        }}
      />
      {unit && <span className="unit-input__unit">{unit}</span>}
    </span>
  );
}
