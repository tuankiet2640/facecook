import { useEffect, useRef, useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { postsApi, mediaApi } from "@/lib/api";
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

type MediaPreview = { file: File; objectUrl: string; uploaded?: string };

function CreatePostForm({ onCreated }: { onCreated: (p: import("@/types/api").Post) => void }) {
  const [content, setContent] = useState("");
  const [focused, setFocused] = useState(false);
  const [media, setMedia] = useState<MediaPreview[]>([]);
  const [uploading, setUploading] = useState(false);
  const fileRef = useRef<HTMLInputElement>(null);

  const { mutate, isPending } = useMutation({
    mutationFn: async () => {
      // Upload any files that haven't been uploaded yet
      const urls: string[] = [];
      for (const m of media) {
        if (m.uploaded) {
          urls.push(m.uploaded);
        } else {
          const url = await mediaApi.upload(m.file);
          urls.push(url);
        }
      }
      return postsApi.create({ content, media_urls: urls });
    },
    onSuccess: (post) => {
      setContent("");
      setFocused(false);
      media.forEach((m) => URL.revokeObjectURL(m.objectUrl));
      setMedia([]);
      onCreated(post);
    },
  });

  function pickFiles(files: FileList | null) {
    if (!files) return;
    const next: MediaPreview[] = Array.from(files)
      .filter((f) => f.type.startsWith("image/") || f.type.startsWith("video/"))
      .slice(0, 4 - media.length) // max 4 attachments
      .map((file) => ({ file, objectUrl: URL.createObjectURL(file) }));
    setMedia((prev) => [...prev, ...next]);
    setFocused(true);
  }

  function removeMedia(idx: number) {
    setMedia((prev) => {
      URL.revokeObjectURL(prev[idx].objectUrl);
      return prev.filter((_, i) => i !== idx);
    });
  }

  const canSubmit = (content.trim().length > 0 || media.length > 0) && !isPending && !uploading;

  return (
    <div className={cn(
      "bg-surface-raised border rounded-xl p-4 transition-colors",
      focused ? "border-accent/50" : "border-surface-border",
    )}>
      <textarea
        value={content}
        onChange={(e) => setContent(e.target.value)}
        onFocus={() => setFocused(true)}
        onBlur={() => !content && media.length === 0 && setFocused(false)}
        placeholder="What's on your mind?"
        rows={focused ? 3 : 1}
        className="w-full bg-transparent text-text-primary placeholder:text-text-muted text-sm resize-none focus:outline-none transition-all"
      />

      {/* Media previews */}
      {media.length > 0 && (
        <div className="mt-3 grid grid-cols-2 gap-2">
          {media.map((m, i) => (
            <div key={m.objectUrl} className="relative rounded-lg overflow-hidden bg-surface-overlay aspect-video">
              {m.file.type.startsWith("video/") ? (
                <video src={m.objectUrl} className="w-full h-full object-cover" muted />
              ) : (
                <img src={m.objectUrl} alt="" className="w-full h-full object-cover" />
              )}
              <button
                onClick={() => removeMedia(i)}
                className="absolute top-1 right-1 bg-black/60 text-white rounded-full w-5 h-5 flex items-center justify-center text-xs hover:bg-black/80"
              >
                ×
              </button>
            </div>
          ))}
        </div>
      )}

      {focused && (
        <div className="flex items-center gap-2 mt-3">
          {/* Media picker */}
          <input
            ref={fileRef}
            type="file"
            accept="image/*,video/*"
            multiple
            className="hidden"
            onChange={(e) => pickFiles(e.target.files)}
          />
          {media.length < 4 && (
            <button
              type="button"
              // preventDefault on mousedown keeps focus on the textarea so its
              // onBlur doesn't collapse the form before our click handler runs.
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => fileRef.current?.click()}
              className="text-text-muted hover:text-accent transition-colors p-1"
              title="Attach image or video"
            >
              <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="8.5" cy="8.5" r="1.5"/>
                <polyline points="21 15 16 10 5 21"/>
              </svg>
            </button>
          )}
          <div className="ml-auto">
            <button
              onClick={() => mutate()}
              disabled={!canSubmit}
              className="bg-accent hover:bg-accent-hover disabled:opacity-40 text-white text-sm font-medium px-4 py-1.5 rounded-lg transition-colors"
            >
              {isPending || uploading ? "Posting…" : "Post"}
            </button>
          </div>
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
