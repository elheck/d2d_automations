import { QueryClient } from "@tanstack/react-query";
import { createMemoryHistory } from "@tanstack/react-router";
import { render } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "../App";
import { setUnauthenticatedHandler } from "../api/client";
import { keys } from "../api/queries";
import { createAppRouter } from "../router";

/** The whole app at `path`, as the browser would show it. */
export function renderApp(path = "/") {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  setUnauthenticatedHandler(() => queryClient.setQueryData(keys.me, null));
  const router = createAppRouter(createMemoryHistory({ initialEntries: [path] }));
  const user = userEvent.setup();
  const view = render(<App queryClient={queryClient} router={router} />);
  return { ...view, router, queryClient, user };
}
