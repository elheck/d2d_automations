import "@testing-library/jest-dom/vitest";
import { cleanup, configure } from "@testing-library/react";
import { afterEach } from "vitest";
import { resetSeenVersion } from "../inventoryVersion";

// The first render of a whole page can take over a second on a busy machine
// (CI, or a build running alongside); findBy… and waitFor wait up to 3 s.
configure({ asyncUtilTimeout: 3000 });

afterEach(() => {
  cleanup();
  resetSeenVersion();
});
