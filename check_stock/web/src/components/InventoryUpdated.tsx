// Shown on every page when the inventory data changed after the page was
// loaded (someone imported): the pages keep the old figures until the user
// asks for the new ones, so nothing jumps while someone reads a table.

import { useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { useDataInfo } from "../api/queries";
import { markSeen, useSeenVersion } from "../inventoryVersion";

export function InventoryUpdated() {
  const { data } = useDataInfo();
  const seen = useSeenVersion();
  const queryClient = useQueryClient();
  useEffect(() => {
    if (data && seen === null) markSeen(data.inventory_version);
  }, [data, seen]);

  if (!data || seen === null || data.inventory_version <= seen) return null;
  const at = data.updated_at ? ` at ${new Date(data.updated_at).toLocaleTimeString()}` : "";
  return (
    <div className="update-banner" role="status">
      <span>The inventory was updated{at}. This page still shows the data from before.</span>
      <button
        type="button"
        className="button"
        onClick={() => {
          markSeen(data.inventory_version);
          void queryClient.invalidateQueries();
        }}
      >
        Show new data
      </button>
    </div>
  );
}
