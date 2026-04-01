import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "react-router-dom";
import { usersApi } from "@/lib/api";
import { qk } from "@/lib/queryKeys";
import { timeAgo, initials, hashColor } from "@/lib/utils";
import type { Post } from "@/types/api";

export function PostCard({ post }: { post: Post }) {
  const navigate = useNavigate();

  const { data: author } = useQuery({
    queryKey: qk.user(post.author_id),
    queryFn: () => usersApi.get(post.author_id),
    staleTime: 60_000,
    // Use pre-hydrated author if attached to the post object.
    initialData: post.author,
  });

  const name = author?.display_name ?? author?.username ?? "Unknown";

  return (
    <article className="bg-surface-raised border border-surface-border rounded-xl p-4 space-y-3 hover:border-surface-overlay transition-colors">
      {/* Author row */}
      <div className="flex items-center gap-3">
        <button
          onClick={() => navigate(`/profile/${post.author_id}`)}
          className="flex items-center gap-3 hover:opacity-80 transition-opacity"
        >
          <div
            className="w-9 h-9 rounded-full flex items-center justify-center text-xs font-bold text-white shrink-0"
            style={{ backgroundColor: hashColor(post.author_id) }}
          >
            {author?.avatar_url
              ? <img src={author.avatar_url} className="w-full h-full rounded-full object-cover" alt={name} />
              : initials(name)
            }
          </div>
          <div className="text-left">
            <p className="text-sm font-semibold text-text-primary leading-tight">{name}</p>
            {author?.username && (
              <p className="text-xs text-text-muted">@{author.username}</p>
            )}
          </div>
        </button>
        <time className="ml-auto text-xs text-text-muted shrink-0">
          {timeAgo(post.created_at)}
        </time>
      </div>

      {/* Content */}
      <p className="text-sm text-text-primary whitespace-pre-wrap break-words leading-relaxed">
        {post.content}
      </p>

      {/* Tags */}
      {post.tags.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {post.tags.map((tag) => (
            <span
              key={tag}
              className="text-xs text-accent bg-accent-muted/30 px-2 py-0.5 rounded-full"
            >
              #{tag}
            </span>
          ))}
        </div>
      )}
    </article>
  );
}
