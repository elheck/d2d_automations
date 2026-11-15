// Server data as TanStack Query hooks. Each mutation refreshes what it
// changed; logging in or out resets everything.

import { useInfiniteQuery, useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { api, ApiError, download } from "./client";
import type {
  AuditPage,
  EvHistory,
  LoadRequest,
  PreconSettings,
  PreconSettingsView,
  PreconStatusView,
  ProductDetail,
  RankingView,
  BuyHelperSettings,
  BuyOffer,
  BuyOfferParams,
  ChangePasswordRequest,
  CreateUserRequest,
  LoginRequest,
  MeResponse,
  PermissionInfo,
  SingleCard,
  UpdateUserRequest,
  UserView,
} from "./types";

export const keys = {
  me: ["me"] as const,
  permissions: ["permissions"] as const,
  users: ["users"] as const,
  audit: ["audit"] as const,
  singles: ["singles"] as const,
  buyHelperSettings: ["buy-helper-settings"] as const,
  buyOffer: ["buy-offer"] as const,
  preconStatus: ["precon-status"] as const,
  preconSettings: ["precon-settings"] as const,
  precon: ["precon"] as const,
};

/** The logged-in user; `null` when not logged in. */
export function useMe() {
  return useQuery({
    queryKey: keys.me,
    queryFn: async (): Promise<MeResponse | null> => {
      try {
        return await api.get<MeResponse>("/api/auth/me");
      } catch (e) {
        if (e instanceof ApiError && e.status === 401) return null;
        throw e;
      }
    },
    staleTime: 60_000,
    retry: false,
  });
}

/** Forget everything about the previous user. */
export function resetSession(queryClient: QueryClient, me: MeResponse | null): void {
  queryClient.removeQueries({ predicate: (q) => q.queryKey[0] !== keys.me[0] });
  queryClient.setQueryData(keys.me, me);
}

export function useLogin() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (credentials: LoginRequest) => api.post<MeResponse>("/api/auth/login", credentials),
    onSuccess: (me) => resetSession(queryClient, me),
  });
}

export function useLogout() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => api.post<void>("/api/auth/logout"),
    onSettled: () => resetSession(queryClient, null),
  });
}

export function useChangePassword() {
  return useMutation({
    mutationFn: (body: ChangePasswordRequest) => api.post<void>("/api/auth/password", body),
  });
}

/** Every permission with label, description, group and requirements. */
export function usePermissions() {
  return useQuery({
    queryKey: keys.permissions,
    queryFn: () => api.get<PermissionInfo[]>("/api/permissions"),
    staleTime: Infinity,
  });
}

export function useUsers() {
  return useQuery({
    queryKey: keys.users,
    queryFn: () => api.get<UserView[]>("/api/users"),
  });
}

function useUserMutation<V, R>(mutationFn: (vars: V) => Promise<R>) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn,
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: keys.users });
      // Changing one's own account changes what the app may show.
      await queryClient.invalidateQueries({ queryKey: keys.me });
      await queryClient.invalidateQueries({ queryKey: keys.audit });
    },
  });
}

export function useCreateUser() {
  return useUserMutation((body: CreateUserRequest) => api.post<UserView>("/api/users", body));
}

export function useUpdateUser() {
  return useUserMutation(({ id, changes }: { id: number; changes: UpdateUserRequest }) =>
    api.patch<UserView>(`/api/users/${id}`, changes),
  );
}

export function useSetPassword() {
  return useUserMutation(({ id, password }: { id: number; password: string }) =>
    api.post<void>(`/api/users/${id}/password`, { password }),
  );
}

export function useDeleteUser() {
  return useUserMutation((id: number) => api.delete<void>(`/api/users/${id}`));
}

export const AUDIT_PAGE_SIZE = 50;

/** The audit log, newest first, one page at a time. */
export function useAuditLog() {
  return useInfiniteQuery({
    queryKey: keys.audit,
    queryFn: ({ pageParam }) =>
      api.get<AuditPage>(
        `/api/audit?limit=${AUDIT_PAGE_SIZE}${pageParam === undefined ? "" : `&before_id=${pageParam}`}`,
      ),
    initialPageParam: undefined as number | undefined,
    getNextPageParam: (last) => last.next_before_id ?? undefined,
  });
}

/** One card lookup of the singles listing, as typed. */
export interface SingleLookup {
  input: string;
  defaultSet: string;
  /** The printing it resolves to (cache key: same card, same answer). */
  setCode: string;
  collectorNumber: string;
}

/** The card behind `lookup`; nothing is fetched while it is `null`. */
export function useSingleCard(lookup: SingleLookup | null) {
  return useQuery({
    queryKey: [...keys.singles, lookup?.setCode, lookup?.collectorNumber],
    queryFn: () => {
      if (!lookup) throw new Error("no card to look up");
      const query = new URLSearchParams({ input: lookup.input, default_set: lookup.defaultSet });
      return api.get<SingleCard>(`/api/singles/lookup?${query.toString()}`);
    },
    enabled: lookup !== null,
    // Prices change once a day; a few minutes avoid asking again while listing.
    staleTime: 5 * 60_000,
  });
}

/** The user's buy helper settings (stored on the server) and the defaults. */
export function useBuyHelperSettings() {
  return useQuery({
    queryKey: keys.buyHelperSettings,
    queryFn: () => api.get<BuyHelperSettings>(BUY_HELPER_SETTINGS),
    staleTime: Infinity,
  });
}

const BUY_HELPER_SETTINGS = "/api/buy-helper/settings";

/**
 * Saves settings while the page is closing (`keepalive`: the browser finishes
 * the request after the page is gone). Errors cannot be shown any more.
 */
export function saveBuyHelperSettingsOnExit(params: BuyOfferParams): void {
  void api.put(BUY_HELPER_SETTINGS, params, { keepalive: true }).catch(() => undefined);
}

export function useSaveBuyHelperSettings() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (params: BuyOfferParams) => api.put<BuyHelperSettings>(BUY_HELPER_SETTINGS, params),
    onSuccess: async (settings) => {
      queryClient.setQueryData(keys.buyHelperSettings, settings);
      await queryClient.invalidateQueries({ queryKey: keys.audit });
    },
  });
}

/** A card list loaded in the browser; `version` changes with every new file. */
export interface CardListFile {
  name: string;
  csv: string;
  version: number;
}

/**
 * The offer for `file` under `params` (nothing is asked while either is
 * missing). The file's text is not part of the cache key, only its version.
 */
export function useBuyOffer(file: CardListFile | null, params: BuyOfferParams | null) {
  return useQuery({
    queryKey: [...keys.buyOffer, file?.version, params],
    queryFn: () => {
      if (!file || !params) throw new Error("no card list");
      return api.post<BuyOffer>("/api/buy-helper/offer", { csv: file.csv, params });
    },
    enabled: file !== null && params !== null,
    // While new settings are being valued, keep showing the last offer for
    // the same file (never one of a previous file).
    placeholderData: (previous, previousQuery) => (previousQuery?.queryKey[1] === file?.version ? previous : undefined),
    staleTime: Infinity,
    gcTime: 60_000,
  });
}

/** The offer as a CSV file. */
export function useExportOffer() {
  return useMutation({
    mutationFn: ({ file, params }: { file: CardListFile; params: BuyOfferParams }) =>
      download("/api/buy-helper/export", { csv: file.csv, params }, "buy_offer.csv"),
  });
}

// ── Precon EV ────────────────────────────────────────────────────────────

/** How often the status is asked for while a load or the sales history runs. */
export const PRECON_POLL_MS = 1000;

/** What is loaded and what the background jobs are doing (polled while they run). */
export function usePreconStatus() {
  return useQuery({
    queryKey: keys.preconStatus,
    queryFn: () => api.get<PreconStatusView>("/api/precon-ev/status"),
    refetchInterval: (query) => {
      const status = query.state.data;
      const running = status && (status.load.state === "running" || status.sales.state === "running");
      return running ? PRECON_POLL_MS : false;
    },
  });
}

export function useStartPreconLoad() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: LoadRequest) => api.post<PreconStatusView>("/api/precon-ev/load", body),
    onSuccess: (status) => queryClient.setQueryData(keys.preconStatus, status),
  });
}

const PRECON_SETTINGS = "/api/precon-ev/settings";

export function usePreconSettings() {
  return useQuery({
    queryKey: keys.preconSettings,
    queryFn: () => api.get<PreconSettingsView>(PRECON_SETTINGS),
    staleTime: Infinity,
  });
}

export function useSavePreconSettings() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (settings: PreconSettings) => api.put<PreconSettingsView>(PRECON_SETTINGS, settings),
    // The detail page reads the settings too: show them there at once.
    onMutate: (settings) => {
      queryClient.setQueryData<PreconSettingsView>(keys.preconSettings, (old) => (old ? { ...old, settings } : old));
    },
    onSuccess: async (view) => {
      queryClient.setQueryData(keys.preconSettings, view);
      await queryClient.invalidateQueries({ queryKey: keys.audit });
    },
  });
}

/** Saves settings while the page is closing (see {@link saveBuyHelperSettingsOnExit}). */
export function savePreconSettingsOnExit(settings: PreconSettings): void {
  void api.put(PRECON_SETTINGS, settings, { keepalive: true }).catch(() => undefined);
}

/** The query string of the pricing parameters. */
export function evQuery(s: PreconSettings): URLSearchParams {
  return new URLSearchParams({
    field: s.field,
    sealed_field: s.sealed_field,
    cutoff: String(s.cutoff),
    foil_needs_sales: String(s.foil_needs_sales),
  });
}

/** The ranking for `settings` and `filter`; `version` = loaded data version (null: nothing loaded). */
export function usePreconRanking(settings: PreconSettings | null, filter: string, version: number | null) {
  return useQuery({
    queryKey: [...keys.precon, "ranking", version, settings, filter],
    queryFn: () => {
      if (!settings) throw new Error("no settings");
      const query = evQuery(settings);
      query.set("sort", settings.sort);
      query.set("desc", String(settings.desc));
      query.set("filter", filter);
      query.set("hide_no_decklist", String(settings.hide_no_decklist));
      return api.get<RankingView>(`/api/precon-ev/ranking?${query.toString()}`);
    },
    enabled: settings !== null && version !== null,
    placeholderData: (previous) => previous,
    staleTime: Infinity,
  });
}

export function usePreconProduct(id: number, settings: PreconSettings | null, version: number | null) {
  return useQuery({
    queryKey: [...keys.precon, "product", id, version, settings && evQuery(settings).toString()],
    queryFn: () => {
      if (!settings) throw new Error("no settings");
      return api.get<ProductDetail>(`/api/precon-ev/products/${id}?${evQuery(settings).toString()}`);
    },
    enabled: settings !== null && version !== null,
    placeholderData: (previous, previousQuery) => (previousQuery?.queryKey[2] === id ? previous : undefined),
    staleTime: Infinity,
  });
}

export function usePreconHistory(id: number, settings: PreconSettings | null, version: number | null) {
  return useQuery({
    queryKey: [...keys.precon, "history", id, version, settings && evQuery(settings).toString()],
    queryFn: () => {
      if (!settings) throw new Error("no settings");
      return api.get<EvHistory>(`/api/precon-ev/products/${id}/history?${evQuery(settings).toString()}`);
    },
    enabled: settings !== null && version !== null,
    placeholderData: (previous, previousQuery) => (previousQuery?.queryKey[2] === id ? previous : undefined),
    staleTime: Infinity,
    retry: false,
  });
}

/** Whether the user holds a permission (the server checks again). */
export function can(me: MeResponse | null | undefined, permission: string): boolean {
  return me?.permissions.includes(permission) ?? false;
}
