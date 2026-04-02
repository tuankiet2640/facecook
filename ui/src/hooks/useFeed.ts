import { useInfiniteQuery, useQuery, useQueryClient } from "@tanstack/react-query";
import { feedApi, postsApi } from "@/lib/api";
import { qk } from "@/lib/queryKeys";
import type { Post } from "@/types/api";

/**
 * Two-query feed architecture:
 *
 * 1. Infinite feed query — fetches pages of { post_id, score } pairs.
 *    Cursor = score (timestamp_ms) of the last item on the previous page.
 *
 * 2. Batch post hydration — once all post_ids across all pages are known,
 *    a single /posts/batch call fetches full post content. TanStack Query
 *    caches individual posts so re-renders don't re-fetch.
 *
 * This keeps the feed hot-path (Redis sorted set) separate from post
 * content (PostgreSQL) — the feed service stays a pure pointer store.
 */
export function useFeed() {
  const queryClient = useQueryClient();

  // ── Step 1: Infinite cursor pagination ────────────────────────────────────
  const feedQuery = useInfiniteQuery({
    queryKey: qk.feed(),
    queryFn: ({ pageParam }) =>
      feedApi.get({ limit: 20, before_score: pageParam as number | undefined }),
    initialPageParam: undefined as number | undefined,
    getNextPageParam: (lastPage) =>
      lastPage.has_more ? (lastPage.next_cursor ?? undefined) : undefined,
    staleTime: 30_000,
  });

  // Collect all post_ids across every loaded page.
  const allPostIds = feedQuery.data?.pages
    .flatMap((p) => p.items.map((i) => i.post_id)) ?? [];

  // ── Step 2: Batch hydration ────────────────────────────────────────────────
  const postsQuery = useQuery({
    queryKey: qk.posts(allPostIds),
    queryFn: () => postsApi.batch(allPostIds),
    enabled: allPostIds.length > 0,
    staleTime: 60_000,
    select: (posts): Record<string, Post> =>
      Object.fromEntries(posts.map((p) => [p.id, p])),
  });

  // Ordered list of fully-hydrated posts for rendering.
  const posts: Post[] = allPostIds
    .map((id) => postsQuery.data?.[id])
    .filter((p): p is Post => p !== undefined);

  return {
    posts,
    isLoading: feedQuery.isLoading,
    isError: feedQuery.isError,
    hasMore: feedQuery.hasNextPage,
    isFetchingMore: feedQuery.isFetchingNextPage,
    fetchMore: feedQuery.fetchNextPage,

    // Prepend a newly created post to the top without invalidating the feed.
    prependPost: (post: Post) => {
      queryClient.setQueryData(qk.posts([post.id]), (old: Record<string, Post> | undefined) => ({
        ...old,
        [post.id]: post,
      }));
      queryClient.setQueryData(
        qk.feed(),
        (old: typeof feedQuery.data | undefined) => {
          if (!old) return old;
          const newItem = { post_id: post.id, score: Date.now() };
          return {
            ...old,
            pages: [
              { ...old.pages[0], items: [newItem, ...old.pages[0].items] },
              ...old.pages.slice(1),
            ],
          };
        },
      );
    },
  };
}
