// Server data as TanStack Query hooks. Each mutation refreshes what it
// changed; logging in or out resets everything.

import { useInfiniteQuery, useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { api, ApiError, download } from "./client";
import type {
  AuditPage,
  OutputRequest,
  OutputText,
  PickedResult,
  PickingRequest,
  PickingView,
  StockCheckRequest,
  StockCheckView,
  UpdateCsvRequest,
  CardHistory,
  DiscardRequest,
  DiscardResult,
  DiscardSummary,
  SearchView,
  BinSort,
  BinsView,
  FragmentedView,
  MoveListCreated,
  MoveListRequest,
  MoveListView,
  PlanSummary,
  TickResult,
  DigestView,
  InventoryConfirmRequest,
  InventoryFileRequest,
  InventoryImportResult,
  InventoryPreview,
  InventoryVersion,
  StockAnalysisView,
  MispricingSortKey,
  MispricingView,
  VerdictFilterKey,
  MarketStatusView,
  MoverDirectionKey,
  MoverSortKey,
  MoversView,
  PriceColumn,
  DataInfo,
  RestockSortKey,
  RestockView,
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
  dataInfo: ["data-info"] as const,
  restock: ["restock"] as const,
  marketStatus: ["market-status"] as const,
  movers: ["movers"] as const,
  mispricing: ["mispricing"] as const,
  stockAnalysis: ["stock-analysis"] as const,
  importHistory: ["import-history"] as const,
  bins: ["bins"] as const,
  fragmented: ["bins-fragmented"] as const,
  consolidation: ["bins-consolidation"] as const,
  moveList: ["move-list"] as const,
  search: ["search"] as const,
  cardHistory: ["card-history"] as const,
  discards: ["discards"] as const,
  picking: ["picking"] as const,
  digest: ["digest"] as const,
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

// ── Reports ──────────────────────────────────────────────────────────────

/** How current the inventory data is (shown above the reports). */
/** How current the inventory data is; polled so open pages notice an import. */
export const DATA_INFO_POLL_MS = 60_000;

export function useDataInfo() {
  return useQuery({
    queryKey: keys.dataInfo,
    queryFn: () => api.get<DataInfo>("/api/data-info"),
    staleTime: DATA_INFO_POLL_MS,
    refetchInterval: DATA_INFO_POLL_MS,
  });
}

/** The latest changes of the inventory data (imports, restores, copies). */
export function useImportHistory() {
  return useQuery({
    queryKey: keys.importHistory,
    queryFn: () => api.get<InventoryVersion[]>("/api/inventory/imports"),
  });
}

/** Checks an export against the current data (nothing is written yet). */
export function usePreviewImport() {
  return useMutation({
    mutationFn: (file: InventoryFileRequest) => api.post<InventoryPreview>("/api/inventory/preview", file),
  });
}

/** Imports the checked export (uploaded again); every page's data changes. */
export function useConfirmImport() {
  return useMutation({
    mutationFn: (request: InventoryConfirmRequest) =>
      api.post<InventoryImportResult>("/api/inventory/confirm", request),
  });
}

/** Drops the user's check. */
export function useDiscardImport() {
  return useMutation({ mutationFn: () => api.delete<void>("/api/inventory/pending") });
}

/**
 * The start page's "since your last visit" digest. A POST (it records the
 * visit day), asked once per page load.
 */
export function useDigest() {
  return useQuery({
    queryKey: keys.digest,
    queryFn: () => api.post<DigestView>("/api/digest"),
    staleTime: Infinity,
    refetchOnWindowFocus: false,
    retry: false,
  });
}

export interface RestockParams {
  minCopies: number;
  sort: RestockSortKey;
  desc: boolean;
}

export function restockQuery(p: RestockParams): string {
  return new URLSearchParams({ min_copies: String(p.minCopies), sort: p.sort, desc: String(p.desc) }).toString();
}

/** The restock list; nothing is asked while `params` is `null` (invalid input). */
export function useRestock(params: RestockParams | null) {
  return useQuery({
    queryKey: [...keys.restock, params],
    queryFn: () => {
      if (!params) throw new Error("no parameters");
      return api.get<RestockView>(`/api/restock?${restockQuery(params)}`);
    },
    enabled: params !== null,
    placeholderData: (previous) => previous,
  });
}

/** The restock list as a buy-list CSV. */
export function useExportRestock() {
  return useMutation({
    mutationFn: (params: RestockParams) =>
      download(`/api/restock/export?${restockQuery(params)}`, undefined, "restock_buy_list.csv"),
  });
}

/** Each report reaches the shared market data through its own routes (its own permission). */
export type MarketBase = "/api/movers" | "/api/mispricing";

/** The shared market data: job and what is loaded; polled while it loads. */
export function useMarketStatus(base: MarketBase) {
  return useQuery({
    queryKey: [...keys.marketStatus, base],
    queryFn: () => api.get<MarketStatusView>(`${base}/status`),
    refetchInterval: (query) => (query.state.data?.job.state === "running" ? PRECON_POLL_MS : false),
  });
}

export function useStartMarketLoad(base: MarketBase) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (refresh: boolean) => api.post<MarketStatusView>(`${base}/load`, { refresh }),
    onSuccess: (status) => queryClient.setQueryData([...keys.marketStatus, base], status),
  });
}

export interface MoversParams {
  field: PriceColumn;
  minPrice: number;
  minAge: number;
  direction: MoverDirectionKey;
  sort: MoverSortKey;
  desc: boolean;
}

/** The movers for `params`; `version` = loaded market data (null: nothing loaded). */
export function useMovers(params: MoversParams | null, version: number | null) {
  return useQuery({
    queryKey: [...keys.movers, version, params],
    queryFn: () => {
      if (!params) throw new Error("no parameters");
      const query = new URLSearchParams({
        field: params.field,
        min_price: String(params.minPrice),
        min_age: String(params.minAge),
        direction: params.direction,
        sort: params.sort,
        desc: String(params.desc),
      });
      return api.get<MoversView>(`/api/movers?${query.toString()}`);
    },
    enabled: params !== null && version !== null,
    placeholderData: (previous) => previous,
    staleTime: Infinity,
  });
}

export interface MispricingParams {
  field: PriceColumn;
  threshold: number;
  filter: VerdictFilterKey;
  sort: MispricingSortKey;
  desc: boolean;
}

/** The mispricing report for `params`; `version` = loaded market data (null: nothing loaded). */
export function useMispricing(params: MispricingParams | null, version: number | null) {
  return useQuery({
    queryKey: [...keys.mispricing, version, params],
    queryFn: () => {
      if (!params) throw new Error("no parameters");
      const query = new URLSearchParams({
        field: params.field,
        threshold: String(params.threshold),
        filter: params.filter,
        sort: params.sort,
        desc: String(params.desc),
      });
      return api.get<MispricingView>(`/api/mispricing?${query.toString()}`);
    },
    enabled: params !== null && version !== null,
    placeholderData: (previous) => previous,
    staleTime: Infinity,
  });
}

/** The stock analysis (money figures only when the server sends them). */
export function useStockAnalysis() {
  return useQuery({
    queryKey: keys.stockAnalysis,
    queryFn: () => api.get<StockAnalysisView>("/api/stock-analysis"),
    staleTime: 60_000,
  });
}

/** Whether the user holds a permission (the server checks again). */
export function can(me: MeResponse | null | undefined, permission: string): boolean {
  return me?.permissions.includes(permission) ?? false;
}

export interface BinParams {
  minFree: number;
  sort: BinSort;
}

const binQuery = (p: BinParams) => `min_free=${p.minFree}&sort=${p.sort}`;

/** The bins of the last import with their fill (null parameters: wait). */
export function useBins(params: BinParams | null) {
  return useQuery({
    queryKey: [...keys.bins, params],
    queryFn: () => {
      if (!params) throw new Error("no parameters");
      return api.get<BinsView>(`/api/bins?${binQuery(params)}`);
    },
    enabled: params !== null,
    placeholderData: (previous) => previous,
  });
}

/** The bin table as CSV. */
export function useExportBins() {
  return useMutation({
    mutationFn: (params: BinParams) => download(`/api/bins/export?${binQuery(params)}`, undefined, "bin_analysis.csv"),
  });
}

/** Variants spread over several bins, with what gathering them takes. */
export function useFragmented() {
  return useQuery({
    queryKey: keys.fragmented,
    queryFn: () => api.get<FragmentedView>("/api/bins/fragmented"),
  });
}

/** What emptying the bins filled up to `threshold` cards would take. */
export function useConsolidationSummary(threshold: number | null) {
  return useQuery({
    queryKey: [...keys.consolidation, threshold],
    queryFn: () => {
      if (threshold === null) throw new Error("no threshold");
      return api.get<PlanSummary>(`/api/bins/consolidation?threshold=${threshold}`);
    },
    enabled: threshold !== null,
    placeholderData: (previous) => previous,
    retry: false,
  });
}

/** The user's move list (`null`: none). */
export function useMoveList() {
  return useQuery({
    queryKey: keys.moveList,
    queryFn: () => api.get<MoveListView | null>("/api/bins/list"),
  });
}

/** Makes (or makes again) the user's move list. */
export function useCreateMoveList() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (request: MoveListRequest) => api.post<MoveListCreated>("/api/bins/list", request),
    onSuccess: (created) => queryClient.setQueryData(keys.moveList, created.list),
  });
}

/**
 * Ticks or unticks moves. The list shows the change at once (walking
 * through the bins with a phone); a refusal puts the old state back.
 */
export function useTickMoves() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (request: { ids: string[]; done: boolean }) => api.put<TickResult>("/api/bins/list/ticks", request),
    onMutate: async ({ ids, done }) => {
      await queryClient.cancelQueries({ queryKey: keys.moveList });
      const before = queryClient.getQueryData<MoveListView | null>(keys.moveList);
      if (before) {
        const changed = new Set(ids);
        const moves = before.moves.map((m) => (changed.has(m.id) ? { ...m, done } : m));
        queryClient.setQueryData<MoveListView>(keys.moveList, {
          ...before,
          moves,
          ticked: moves.filter((m) => m.done).length,
        });
      }
      return { before };
    },
    onError: (_error, _request, context) => {
      if (context?.before !== undefined) queryClient.setQueryData(keys.moveList, context.before);
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: keys.moveList }),
  });
}

export function useDiscardMoveList() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => api.delete<void>("/api/bins/list"),
    onSuccess: () => queryClient.setQueryData(keys.moveList, null),
  });
}

/** The ticked piles as a Cardmarket stock-update CSV. */
export function useExportMoves() {
  return useMutation({
    mutationFn: (fallbackName: string) => download("/api/bins/list/export", undefined, fallbackName),
  });
}

export type SearchField = "name" | "set" | "condition" | "language" | "location" | "rarity" | "price" | "comment";

export interface SearchParams {
  q: string;
  fields: SearchField[];
  caseSensitive: boolean;
  allLanguages: boolean;
  page: number;
  perPage: number;
}

export function searchQuery(p: SearchParams): string {
  const query = new URLSearchParams({
    q: p.q,
    fields: p.fields.join(","),
    case_sensitive: String(p.caseSensitive),
    all_languages: String(p.allLanguages),
    page: String(p.page),
    per_page: String(p.perPage),
  });
  return query.toString();
}

/** Piles of the stock matching the search. */
export function useSearch(params: SearchParams) {
  return useQuery({
    queryKey: [...keys.search, params],
    queryFn: () => api.get<SearchView>(`/api/search?${searchQuery(params)}`),
    placeholderData: (previous) => previous,
  });
}

/** Price history of a card (`null`: closed). */
export function useCardHistory(card: { product: number; foil: boolean } | null) {
  return useQuery({
    queryKey: [...keys.cardHistory, card],
    queryFn: () => {
      if (!card) throw new Error("no card");
      return api.get<CardHistory>(`/api/search/history/${card.product}?foil=${card.foil}`);
    },
    enabled: card !== null,
    retry: false,
  });
}

/** Writes piles off; every page's data changes. */
export function useDiscard() {
  return useMutation({
    mutationFn: (request: DiscardRequest) => api.post<DiscardResult>("/api/inventory/discard", request),
  });
}

/** The latest discards. */
export function useDiscardHistory(enabled: boolean) {
  return useQuery({
    queryKey: keys.discards,
    queryFn: () => api.get<DiscardSummary[]>("/api/inventory/discards"),
    enabled,
  });
}

/** The stock-update CSV of discard `id`. */
export function downloadDiscardCsv(id: number) {
  return download(`/api/inventory/discards/${id}/csv`, undefined, `discarded_cards_${id}.csv`);
}

/** Matches a wantslist against the stock. */
export function useStockCheck() {
  return useMutation({
    mutationFn: (request: StockCheckRequest) => api.post<StockCheckView>("/api/stock-checker/check", request),
  });
}

/** Picking list, invoice list or summary of the chosen piles, as text. */
export function useStockOutput() {
  return useMutation({
    mutationFn: (request: OutputRequest) => api.post<OutputText>("/api/stock-checker/output", request),
  });
}

/** The stock-update CSV that takes the chosen piles off Cardmarket. */
export function useStockUpdateCsv() {
  return useMutation({
    mutationFn: (request: UpdateCsvRequest) => download("/api/stock-checker/update-csv", request, "stock_update.csv"),
  });
}

/** The user's picking list (`null`: none). */
export function usePicking() {
  return useQuery({
    queryKey: keys.picking,
    queryFn: () => api.get<PickingView | null>("/api/picking"),
  });
}

export function useCreatePicking() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (request: PickingRequest) => api.post<PickingView>("/api/picking", request),
    onSuccess: (list) => queryClient.setQueryData(keys.picking, list),
  });
}

/** Marks cards as picked at once; a refusal puts the old state back. */
export function useSetPicked() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (request: { piles: string[]; picked: boolean }) =>
      api.put<PickedResult>("/api/picking/picked", request),
    onMutate: async ({ piles, picked }) => {
      await queryClient.cancelQueries({ queryKey: keys.picking });
      const before = queryClient.getQueryData<PickingView | null>(keys.picking);
      if (before) {
        const changed = new Set(piles);
        queryClient.setQueryData<PickingView>(keys.picking, {
          ...before,
          items: before.items.map((i) => (changed.has(i.pile) ? { ...i, picked } : i)),
        });
      }
      return { before };
    },
    onError: (_error, _request, context) => {
      if (context?.before !== undefined) queryClient.setQueryData(keys.picking, context.before);
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: keys.picking }),
  });
}

export function useDiscardPicking() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => api.delete<void>("/api/picking"),
    onSuccess: () => queryClient.setQueryData(keys.picking, null),
  });
}
