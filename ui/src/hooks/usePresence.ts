import { useQuery } from "@tanstack/react-query";
import { presenceApi } from "@/lib/api";
import { qk } from "@/lib/queryKeys";

/**
 * Poll presence for a list of user IDs every 30 seconds.
 * Only enabled when the component is mounted and userIds is non-empty.
 */
export function usePresence(userIds: string[]) {
  return useQuery({
    queryKey: qk.presence(userIds),
    queryFn: () => presenceApi.batch(userIds),
    enabled: userIds.length > 0,
    refetchInterval: 30_000,
    staleTime: 25_000,
    select: (statuses) =>
      Object.fromEntries(statuses.map((s) => [s.user_id, s.online])) as Record<string, boolean>,
  });
}
