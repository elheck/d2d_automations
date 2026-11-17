// Above every report: how current the inventory data is. Until the cutover
// the data is a test copy of the desktop app's database.

import { useDataInfo } from "../api/queries";

export function DataAsOf() {
  const { data } = useDataInfo();
  if (!data) return null;
  if (!data.last_import && !data.copy_taken_at) {
    return <p className="data-as-of">No inventory data yet.</p>;
  }
  return (
    <p className="data-as-of">
      Data as of {data.last_import ?? "—"} (last inventory import)
      {data.test_copy &&
        data.copy_taken_at &&
        ` · test copy of the desktop app's database from ${new Date(data.copy_taken_at).toLocaleString()}`}
    </p>
  );
}
