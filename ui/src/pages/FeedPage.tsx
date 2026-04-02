import { useEffect, useRef, useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { postsApi } from "@/lib/api";
import { useFeed } from "@/hooks/useFeed";
import { PostCard } from "@/components/feed/PostCard";
import { cn } from "@/lib/utils";

export function FeedPage() {
  const { posts, isLoading, isError, hasMore, isFetchingMore, fetchMore, prependPost } = useFeed();
  const sentinelRef = useRef<HTMLDivElement>(null);

  // IntersectionObserver triggers the next page when the sentinel scrolls into view.
  useEffect(() => {
    const el = sentinelRef.current;
    if (!el) return;
    const observer = new IntersectionObserver(
      ([entry]) => { if (entry.isIntersecting && hasMore && !isFetchingMore) fetchMore(); },
      { rootMargin: "300px" },
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, [hasMore, isFetchingMore, fetchMore]);

  return (
    <div className="max-w-xl mx-auto py-6 space-y-4 px-4">
      <CreatePostForm onCreated={prependPost} />

      {isLoading && <FeedSkeleton />}
      {isError && <p className="text-red-400 text-sm text-center py-8">Failed to load feed.</p>}

      {posts.map((post) => (
        <PostCard key={post.id} post={post} />
      ))}

      {/* Scroll sentinel */}
      <div ref={sentinelRef} className="h-1" />
      {isFetchingMore && <p className="text-text-muted text-xs text-center pb-4">Loading more…</p>}
      {!hasMore && posts.length > 0 && (
        <p className="text-text-muted text-xs text-center pb-8">You're all caught up.</p>
      )}
    </div>
  );
}

// ── Create post ───────────────────────────────────────────────────────────────

function CreatePostForm({ onCreated }: { onCreated: (p: import("@/types/api").Post) => void }) {
  const [content, setContent] = useState("");
  const [focused, setFocused] = useState(false);

  const { mutate, isPending } = useMutation({
    mutationFn: () => postsApi.create({ content }),
    onSuccess: (post) => { setContent(""); setFocused(false); onCreated(post); },
  });

  const canSubmit = content.trim().length > 0 && !isPending;

  return (
    <div className={cn(
      "bg-surface-raised border rounded-xl p-4 transition-colors",
      focused ? "border-accent/50" : "border-surface-border",
    )}>
      <textarea
        value={content}
        onChange={(e) => setContent(e.target.value)}
        onFocus={() => setFocused(true)}
        onBlur={() => !content && setFocused(false)}
        placeholder="What's on your mind?"
        rows={focused ? 3 : 1}
        className="w-full bg-transparent text-text-primary placeholder:text-text-muted text-sm resize-none focus:outline-none transition-all"
      />
      {focused && (
        <div className="flex justify-end mt-3">
          <button
            onClick={() => mutate()}
            disabled={!canSubmit}
            className="bg-accent hover:bg-accent-hover disabled:opacity-40 text-white text-sm font-medium px-4 py-1.5 rounded-lg transition-colors"
          >
            {isPending ? "Posting…" : "Post"}
          </button>
        </div>
      )}
    </div>
  );
}

// ── Loading skeleton ──────────────────────────────────────────────────────────

function FeedSkeleton() {
  return (
    <div className="space-y-4">
      {[1, 2, 3].map((i) => (
        <div key={i} className="bg-surface-raised border border-surface-border rounded-xl p-4 animate-pulse space-y-3">
          <div className="flex items-center gap-3">
            <div className="w-9 h-9 rounded-full bg-surface-overlay" />
            <div className="space-y-1.5">
              <div className="h-3 w-24 bg-surface-overlay rounded" />
              <div className="h-2.5 w-16 bg-surface-overlay rounded" />
            </div>
          </div>
          <div className="space-y-2">
            <div className="h-3 bg-surface-overlay rounded w-full" />
            <div className="h-3 bg-surface-overlay rounded w-4/5" />
          </div>
        </div>
      ))}
    </div>
  );
}
