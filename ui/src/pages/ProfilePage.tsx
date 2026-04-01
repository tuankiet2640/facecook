import { useState } from "react";
import { useParams, useNavigate } from "react-router-dom";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { usersApi, chatApi } from "@/lib/api";
import { useAuthStore } from "@/stores/auth";
import { qk } from "@/lib/queryKeys";
import { timeAgo, initials, hashColor, cn } from "@/lib/utils";

export function ProfilePage() {
  const { userId } = useParams<{ userId: string }>();
  const myId = useAuthStore((s) => s.userId);
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const isMe = userId === myId;

  const { data: user, isLoading } = useQuery({
    queryKey: qk.user(userId!),
    queryFn: () => usersApi.get(userId!),
    enabled: !!userId,
  });


  // Check if the current user is following this profile.
  const { data: following } = useQuery({
    queryKey: qk.userFollowing(myId!),
    queryFn: () => usersApi.following(myId!, 200),
    select: (data) => data.data.some((u) => u.id === userId),
    enabled: !!myId && !isMe,
    staleTime: 30_000,
  });

  const { mutate: follow, isPending: followPending } = useMutation({
    mutationFn: () => usersApi.follow(userId!),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: qk.userFollowing(myId!) });
      queryClient.invalidateQueries({ queryKey: qk.user(userId!) });
    },
  });

  const { mutate: unfollow, isPending: unfollowPending } = useMutation({
    mutationFn: () => usersApi.unfollow(userId!),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: qk.userFollowing(myId!) });
      queryClient.invalidateQueries({ queryKey: qk.user(userId!) });
    },
  });

  const { mutate: startChat } = useMutation({
    mutationFn: () => chatApi.createConversation(userId!),
    onSuccess: (conv) => {
      queryClient.invalidateQueries({ queryKey: qk.conversations() });
      navigate(`/chat/${conv.id}`);
    },
  });

  if (isLoading) return <ProfileSkeleton />;
  if (!user) return <p className="text-center mt-16 text-text-muted">User not found.</p>;

  const avatarName = user.display_name ?? user.username;

  return (
    <div className="max-w-xl mx-auto py-8 px-4">
      {/* Avatar + name */}
      <div className="flex items-start gap-5">
        <div
          className="w-20 h-20 rounded-full flex items-center justify-center text-2xl font-bold text-white shrink-0"
          style={{ backgroundColor: hashColor(user.id) }}
        >
          {user.avatar_url
            ? <img src={user.avatar_url} className="w-full h-full rounded-full object-cover" alt={avatarName ?? ""} />
            : initials(avatarName)
          }
        </div>

        <div className="flex-1 min-w-0">
          <h1 className="text-xl font-bold text-text-primary">
            {user.display_name ?? user.username}
          </h1>
          <p className="text-text-secondary text-sm">@{user.username}</p>
          {user.bio && <p className="text-text-primary text-sm mt-2">{user.bio}</p>}

          {/* Stats */}
          <div className="flex gap-5 mt-3 text-sm">
            <span className="text-text-secondary">
              <strong className="text-text-primary">{user.following_count}</strong> Following
            </span>
            <span className="text-text-secondary">
              <strong className="text-text-primary">{user.follower_count}</strong> Followers
            </span>
          </div>
        </div>
      </div>

      {/* Actions */}
      {!isMe && (
        <div className="flex gap-2 mt-5">
          <button
            onClick={() => following ? unfollow() : follow()}
            disabled={followPending || unfollowPending}
            className={cn(
              "px-5 py-2 rounded-lg text-sm font-medium transition-colors disabled:opacity-50",
              following
                ? "bg-surface-overlay border border-surface-border text-text-primary hover:border-red-500 hover:text-red-400"
                : "bg-accent hover:bg-accent-hover text-white",
            )}
          >
            {followPending || unfollowPending ? "…" : following ? "Unfollow" : "Follow"}
          </button>
          <button
            onClick={() => startChat()}
            className="px-5 py-2 rounded-lg text-sm font-medium bg-surface-overlay border border-surface-border text-text-primary hover:border-accent/50 transition-colors"
          >
            Message
          </button>
        </div>
      )}

      {isMe && (
        <div className="mt-5">
          <EditProfileForm user={user} />
        </div>
      )}

      <p className="text-text-muted text-xs mt-6">
        Joined {timeAgo(user.created_at)}
      </p>
    </div>
  );
}

function EditProfileForm({ user }: { user: import("@/types/api").User }) {
  const queryClient = useQueryClient();
  const setUser = useAuthStore((s) => s.setUser);
  const [editing, setEditing] = useState(false);
  const [form, setForm] = useState({
    display_name: user.display_name ?? "",
    bio: user.bio ?? "",
  });

  const { mutate, isPending } = useMutation({
    mutationFn: () => usersApi.update(form),
    onSuccess: (updated) => {
      setUser(updated);
      queryClient.setQueryData(qk.user(user.id), updated);
      setEditing(false);
    },
  });

  if (!editing) {
    return (
      <button
        onClick={() => setEditing(true)}
        className="px-5 py-2 rounded-lg text-sm font-medium bg-surface-overlay border border-surface-border text-text-primary hover:border-accent/50 transition-colors"
      >
        Edit profile
      </button>
    );
  }

  return (
    <div className="space-y-3 bg-surface-raised border border-surface-border rounded-xl p-4">
      <div className="space-y-1">
        <label className="text-xs text-text-secondary">Display name</label>
        <input
          value={form.display_name}
          onChange={(e) => setForm((f) => ({ ...f, display_name: e.target.value }))}
          className="w-full bg-surface-overlay border border-surface-border rounded-lg px-3 py-2 text-sm text-text-primary focus:outline-none focus:border-accent/50"
        />
      </div>
      <div className="space-y-1">
        <label className="text-xs text-text-secondary">Bio</label>
        <textarea
          value={form.bio}
          onChange={(e) => setForm((f) => ({ ...f, bio: e.target.value }))}
          rows={3}
          className="w-full bg-surface-overlay border border-surface-border rounded-lg px-3 py-2 text-sm text-text-primary focus:outline-none focus:border-accent/50 resize-none"
        />
      </div>
      <div className="flex gap-2 justify-end">
        <button onClick={() => setEditing(false)} className="text-sm text-text-secondary hover:text-text-primary px-3 py-1.5">
          Cancel
        </button>
        <button onClick={() => mutate()} disabled={isPending} className="bg-accent hover:bg-accent-hover disabled:opacity-50 text-white text-sm px-4 py-1.5 rounded-lg">
          {isPending ? "Saving…" : "Save"}
        </button>
      </div>
    </div>
  );
}

function ProfileSkeleton() {
  return (
    <div className="max-w-xl mx-auto py-8 px-4 animate-pulse space-y-4">
      <div className="flex items-start gap-5">
        <div className="w-20 h-20 rounded-full bg-surface-overlay" />
        <div className="flex-1 space-y-2 pt-1">
          <div className="h-5 w-36 bg-surface-overlay rounded" />
          <div className="h-3 w-24 bg-surface-overlay rounded" />
          <div className="h-3 w-48 bg-surface-overlay rounded" />
        </div>
      </div>
    </div>
  );
}
