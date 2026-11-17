import type { ReactNode } from "react";
import { can, useMe } from "../api/queries";

/** Shows `children` only to users with `permission` (the server checks too). */
export function RequirePermission({ permission, children }: { permission: string; children: ReactNode }) {
  const { data: me } = useMe();
  if (!can(me, permission)) {
    return (
      <section className="page">
        <h1>No access</h1>
        <p>Your account does not have the permission this page needs. Ask an administrator.</p>
      </section>
    );
  }
  return <>{children}</>;
}
