// Server data as TanStack Query hooks. Each mutation refreshes what it
// changed; logging in or out resets everything.

import { useInfiniteQuery, useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { api, ApiError } from "./client";
import type {
  AuditPage,
  ChangePasswordRequest,
  CreateUserRequest,
  LoginRequest,
  MeResponse,
  PermissionInfo,
  UpdateUserRequest,
  UserView,
} from "./types";

export const keys = {
  me: ["me"] as const,
  permissions: ["permissions"] as const,
  users: ["users"] as const,
  audit: ["audit"] as const,
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

/** Whether the user holds a permission (the server checks again). */
export function can(me: MeResponse | null | undefined, permission: string): boolean {
  return me?.permissions.includes(permission) ?? false;
}
