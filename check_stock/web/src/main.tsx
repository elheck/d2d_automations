import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App, createQueryClient } from "./App";
import { setUnauthenticatedHandler } from "./api/client";
import { keys } from "./api/queries";
import { createAppRouter } from "./router";
import "./styles.css";

const queryClient = createQueryClient();
// A session that ended (expired, logged out elsewhere, account disabled)
// sends the user back to the login page.
setUnauthenticatedHandler(() => queryClient.setQueryData(keys.me, null));
const router = createAppRouter();

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <App queryClient={queryClient} router={router} />
    </StrictMode>,
  );
}
