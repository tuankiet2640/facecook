// Centralised query key factory — ensures cache invalidations are type-safe
// and co-located rather than scattered as magic strings.

export const qk = {
  me: () => ["users", "me"] as const,
  user: (userId: string) => ["users", userId] as const,
  userFollowers: (userId: string) => ["users", userId, "followers"] as const,
  userFollowing: (userId: string) => ["users", userId, "following"] as const,

  feed: () => ["feed"] as const,
  posts: (ids: string[]) => ["posts", "batch", ...ids.sort()] as const,
  post: (postId: string) => ["posts", postId] as const,

  conversations: () => ["chat", "conversations"] as const,
  messages: (convId: string) => ["chat", "messages", convId] as const,

  presence: (userIds: string[]) => ["presence", ...userIds.sort()] as const,
};
