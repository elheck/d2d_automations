// The shared market data of the reports (price movers, mispricing): loads
// today's data when the page opens (the server does nothing when it is
// there) and again after an import changed the stock, offers to fetch
// again, and shows progress and what is loaded.

import { useEffect, useRef } from "react";
import { useStartMarketLoad, type MarketBase } from "../api/queries";
import type { MarketStatusView } from "../api/types";
import { ErrorMessage } from "./ErrorMessage";
import { JobProgress } from "./JobProgress";

export function MarketPanel({ base, status }: { base: MarketBase; status: MarketStatusView }) {
  const start = useStartMarketLoad(base);
  // The `stock_changed` value the last automatic load was asked for: once
  // when the page opens, and again when an import changes the stock.
  const askedFor = useRef<boolean | null>(null);
  useEffect(() => {
    if (!status.stock_changed && askedFor.current === true) askedFor.current = false;
    if (!status.configured || status.job.state === "running") return;
    if (askedFor.current !== null && !(status.stock_changed && askedFor.current === false)) return;
    askedFor.current = status.stock_changed;
    start.mutate(false);
  }, [status.configured, status.job.state, status.stock_changed, start]);

  if (!status.configured) {
    return (
      <p className="error" role="alert">
        No price tracker is configured on the server, so there are no market prices. Start the server with
        --inventory-sync-url.
      </p>
    );
  }
  const loading = status.job.state === "running";
  return (
    <div className="panel precon-load">
      <div className="precon-load__actions">
        <button
          type="button"
          className="button"
          disabled={loading || start.isPending}
          onClick={() => start.mutate(true)}
          title="Fetch today's market prices from the price tracker again"
        >
          Fetch again
        </button>
        <JobProgress status={status.job} />
      </div>
      {status.stock_changed && status.job.state !== "running" && (
        <p className="muted">The stock changed (an inventory import) since these prices were loaded.</p>
      )}
      {status.loaded && (
        <p className="muted">
          Market prices of {status.loaded.day} for {status.loaded.products} products ({status.loaded.cards} cards in
          stock) · loaded {new Date(status.loaded.loaded_at).toLocaleString()}
        </p>
      )}
      {status.job.state === "failed" && (
        <p className="error" role="alert">
          {status.job.error}
        </p>
      )}
      {/* Only a requested refresh explains a refusal; the automatic load stays quiet. */}
      {start.error && start.variables === true && <ErrorMessage error={start.error} />}
    </div>
  );
}
