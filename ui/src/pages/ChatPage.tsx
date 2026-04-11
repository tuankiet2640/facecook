import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useParams, useNavigate } from "react-router-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { chatApi, usersApi } from "@/lib/api";
import { useAuthStore } from "@/stores/auth";
import { useChat } from "@/hooks/useChat";
import { usePresence } from "@/hooks/usePresence";
import { qk } from "@/lib/queryKeys";
import { timeAgo, initials, hashColor, cn } from "@/lib/utils";
import type { Conversation, Message, User } from "@/types/api";

export function ChatPage() {
  const { convId } = useParams<{ convId?: string }>();
  const navigate = useNavigate();
  const myId = useAuthStore((s) => s.userId)!;
  const queryClient = useQueryClient();

  const [messages, setMessages] = useState<Message[]>([]);
  const [presenceMap, setPresenceMap] = useState<Record<string, boolean>>({});
  const bottomRef = useRef<HTMLDivElement>(null);

  // Use a ref so the WS onMessage closure can read the *current* convId without
  // re-creating the callback (and re-connecting the socket) on every navigation.
  const activeConvIdRef = useRef<string | null>(convId ?? null);
  useEffect(() => { activeConvIdRef.current = convId ?? null; }, [convId]);

  // ── WebSocket ──────────────────────────────────────────────────────────────
  const { status, sendMessage } = useChat({
    conversationId: convId ?? null,
    onMessage: useCallback((msg: Message) => {
      // Only append to the local thread if it belongs to the conversation
      // currently being viewed — otherwise we'd pollute the open thread with
      // messages from other conversations that share this WebSocket.
      if (msg.conversation_id === activeConvIdRef.current) {
        setMessages((prev) => {
          if (prev.some((m) => m.id === msg.id)) return prev;
          return [...prev, msg];
        });
      }
      // Always invalidate the conversation list so "last message" preview updates,
      // regardless of which conversation the message belongs to.
      queryClient.invalidateQueries({ queryKey: qk.conversations() });
    }, [queryClient]),
    onPresenceUpdate: useCallback((userId: string, online: boolean, _lastSeen: string | null) => {
      setPresenceMap((p) => ({ ...p, [userId]: online }));
    }, []),
  });

  // ── Conversation list ──────────────────────────────────────────────────────
  const { data: conversations = [] } = useQuery({
    queryKey: qk.conversations(),
    queryFn: () => chatApi.conversations(30),
    staleTime: 10_000,
  });

  // ── Active conversation & history ─────────────────────────────────────────
  const activeConv = conversations.find((c) => c.id === convId);
  const otherUserId = activeConv
    ? activeConv.participant_a === myId ? activeConv.participant_b : activeConv.participant_a
    : null;

  // ── Friends list (people I follow) — sidebar discovery panel ──────────────
  // Cache the raw API envelope ({ data, limit, offset }) — same shape as the
  // existing `usersApi.following` query in ProfilePage so we share the cache
  // entry under qk.userFollowing(myId) without a shape conflict.
  const { data: followingResp } = useQuery({
    queryKey: qk.userFollowing(myId),
    queryFn: () => usersApi.following(myId, 50),
    staleTime: 30_000,
    enabled: !!myId,
  });
  const following = followingResp?.data ?? [];

  // Hide users I already have an active conversation with so the panel acts
  // as a "start a new chat" picker instead of duplicating the inbox.
  const conversationPartnerIds = useMemo(() => {
    const set = new Set<string>();
    conversations.forEach((c) => set.add(c.participant_a === myId ? c.participant_b : c.participant_a));
    return set;
  }, [conversations, myId]);

  const peopleToShow = useMemo(
    () => following.filter((u) => !conversationPartnerIds.has(u.id)),
    [following, conversationPartnerIds],
  );

  const { mutate: startChatWith } = useMutation({
    mutationFn: (otherId: string) => chatApi.createConversation(otherId),
    onSuccess: (conv) => {
      // Mirror ProfilePage: optimistically inject so the new chat panel renders
      // immediately rather than blinking through the empty state.
      queryClient.setQueryData<Conversation[]>(qk.conversations(), (old) => {
        if (!old) return [conv];
        if (old.some((c) => c.id === conv.id)) return old;
        return [conv, ...old];
      });
      queryClient.invalidateQueries({ queryKey: qk.conversations() });
      navigate(`/chat/${conv.id}`);
    },
  });

  const { data: otherUser } = useQuery({
    queryKey: qk.user(otherUserId ?? ""),
    queryFn: () => usersApi.get(otherUserId!),
    enabled: !!otherUserId,
  });

  // Load message history when conversation changes.
  useEffect(() => {
    if (!convId) { setMessages([]); return; }
    let cancelled = false;
    chatApi.messages(convId, { limit: 50 }).then((page) => {
      if (!cancelled) setMessages([...page.data].reverse()); // API returns newest-first
    });
    return () => { cancelled = true; };
  }, [convId]);

  // Scroll to bottom when messages arrive.
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages.length]);

  // Poll presence for the other user.
  const { data: pollPresence } = usePresence(otherUserId ? [otherUserId] : []);
  const isOnline = otherUserId
    ? (presenceMap[otherUserId] ?? pollPresence?.[otherUserId] ?? false)
    : false;

  // ── New conversation shortcut ──────────────────────────────────────────────

  return (
    <div className="flex h-full overflow-hidden">
      {/* ── Conversation list + people picker (left panel) ─────────────────── */}
      <aside className="w-72 shrink-0 border-r border-surface-border flex flex-col">
        <div className="p-4 border-b border-surface-border">
          <h2 className="font-semibold text-text-primary">Messages</h2>
        </div>
        <div className="flex-1 overflow-y-auto">
          {conversations.length === 0 && (
            <p className="text-text-muted text-sm text-center mt-8 px-4">
              No conversations yet.<br />Pick someone below to start one.
            </p>
          )}
          {conversations.map((conv) => (
            <ConversationRow
              key={conv.id}
              conv={conv}
              myId={myId}
              isActive={conv.id === convId}
              onClick={() => navigate(`/chat/${conv.id}`)}
              onAvatarClick={(otherId) => navigate(`/profile/${otherId}`)}
            />
          ))}

          {/* People picker — users I follow that I haven't messaged yet.
              Doubles as a friend list: clicking the avatar jumps to their
              profile, clicking the row starts a new conversation. */}
          {peopleToShow.length > 0 && (
            <div className="mt-4">
              <h3 className="px-4 pb-1 pt-3 text-[11px] font-semibold uppercase tracking-wider text-text-muted">
                People you follow
              </h3>
              {peopleToShow.map((u) => (
                <PersonRow
                  key={u.id}
                  user={u}
                  onMessage={() => startChatWith(u.id)}
                  onProfile={() => navigate(`/profile/${u.id}`)}
                />
              ))}
            </div>
          )}
        </div>
      </aside>

      {/* ── Message thread (main panel) ────────────────────────────────────── */}
      {convId && activeConv ? (
        <div className="flex-1 flex flex-col overflow-hidden">
          {/* Header — clicking the avatar or name opens the other user's profile. */}
          <div className="px-4 py-3 border-b border-surface-border flex items-center gap-3">
            <button
              onClick={() => otherUserId && navigate(`/profile/${otherUserId}`)}
              disabled={!otherUserId}
              className="flex items-center gap-3 hover:opacity-80 transition-opacity text-left disabled:cursor-default"
              title={otherUserId ? "View profile" : undefined}
            >
              <UserAvatar user={otherUser} size="sm" />
              <div>
                <p className="font-medium text-sm text-text-primary">
                  {otherUser?.display_name ?? otherUser?.username ?? "…"}
                </p>
                <p className={cn("text-xs", isOnline ? "text-green-400" : "text-text-muted")}>
                  {isOnline ? "Online" : "Offline"}
                </p>
              </div>
            </button>
            <div className="ml-auto">
              <WsStatusBadge status={status} />
            </div>
          </div>

          {/* Messages */}
          <div className="flex-1 overflow-y-auto px-4 py-4 space-y-1">
            {messages.map((msg) => (
              <MessageBubble key={msg.id} msg={msg} isMine={msg.sender_id === myId} />
            ))}
            <div ref={bottomRef} />
          </div>

          {/* Input */}
          <MessageInput
            onSend={(content) => sendMessage(convId, content)}
            disabled={status !== "connected"}
          />
        </div>
      ) : (
        <div className="flex-1 flex items-center justify-center text-text-muted text-sm">
          Select a conversation or visit a profile to start chatting.
        </div>
      )}
    </div>
  );
}

// ── Sub-components ────────────────────────────────────────────────────────────

function ConversationRow({
  conv, myId, isActive, onClick, onAvatarClick,
}: {
  conv: Conversation;
  myId: string;
  isActive: boolean;
  onClick: () => void;
  onAvatarClick?: (otherId: string) => void;
}) {
  const otherId = conv.participant_a === myId ? conv.participant_b : conv.participant_a;
  const { data: user } = useQuery({
    queryKey: qk.user(otherId),
    queryFn: () => usersApi.get(otherId),
    staleTime: 60_000,
  });

  return (
    <div
      className={cn(
        "w-full px-4 py-3 flex items-center gap-3 text-left transition-colors hover:bg-surface-overlay cursor-pointer",
        isActive && "bg-surface-overlay border-r-2 border-accent",
      )}
      onClick={onClick}
    >
      {/* Avatar gets its own click target so users can jump to profile without
          opening the conversation pane. */}
      <button
        onClick={(e) => { e.stopPropagation(); onAvatarClick?.(otherId); }}
        title="View profile"
        className="hover:opacity-80 transition-opacity"
      >
        <UserAvatar user={user} size="sm" />
      </button>
      <div className="min-w-0 flex-1">
        <p className="text-sm font-medium text-text-primary truncate">
          {user?.display_name ?? user?.username ?? "…"}
        </p>
        {conv.last_message_at && (
          <p className="text-xs text-text-muted truncate">
            {timeAgo(conv.last_message_at)}
          </p>
        )}
      </div>
    </div>
  );
}

/// Sidebar entry for someone you follow but haven't messaged yet.
/// Click the avatar to view their profile, click the row to start a chat.
function PersonRow({
  user, onMessage, onProfile,
}: { user: User; onMessage: () => void; onProfile: () => void }) {
  return (
    <div
      onClick={onMessage}
      className="w-full px-4 py-2.5 flex items-center gap-3 text-left transition-colors hover:bg-surface-overlay cursor-pointer"
    >
      <button
        onClick={(e) => { e.stopPropagation(); onProfile(); }}
        title="View profile"
        className="hover:opacity-80 transition-opacity"
      >
        <UserAvatar user={user} size="sm" />
      </button>
      <div className="min-w-0 flex-1">
        <p className="text-sm font-medium text-text-primary truncate">
          {user.display_name ?? user.username}
        </p>
        <p className="text-xs text-text-muted truncate">@{user.username}</p>
      </div>
    </div>
  );
}

function MessageBubble({ msg, isMine }: { msg: Message; isMine: boolean }) {
  return (
    <div className={cn("flex", isMine ? "justify-end" : "justify-start")}>
      <div
        className={cn(
          "max-w-[70%] rounded-2xl px-3.5 py-2 text-sm",
          isMine
            ? "bg-accent text-white rounded-br-sm"
            : "bg-surface-overlay text-text-primary rounded-bl-sm",
        )}
      >
        <p className="break-words">{msg.content}</p>
        <p className={cn("text-xs mt-0.5", isMine ? "text-white/60 text-right" : "text-text-muted")}>
          {timeAgo(msg.created_at)}
        </p>
      </div>
    </div>
  );
}

function MessageInput({ onSend, disabled }: { onSend: (content: string) => void; disabled: boolean }) {
  const [text, setText] = useState("");

  const submit = () => {
    const trimmed = text.trim();
    if (!trimmed || disabled) return;
    onSend(trimmed);
    setText("");
  };

  return (
    <div className="p-3 border-t border-surface-border flex gap-2">
      <input
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); submit(); } }}
        placeholder={disabled ? "Connecting…" : "Message"}
        disabled={disabled}
        className="flex-1 bg-surface-overlay border border-surface-border rounded-full px-4 py-2 text-sm text-text-primary placeholder:text-text-muted focus:outline-none focus:border-accent/50 disabled:opacity-50 transition-colors"
      />
      <button
        onClick={submit}
        disabled={!text.trim() || disabled}
        className="w-9 h-9 bg-accent hover:bg-accent-hover disabled:opacity-40 text-white rounded-full flex items-center justify-center transition-colors shrink-0"
      >
        <SendIcon />
      </button>
    </div>
  );
}

function WsStatusBadge({ status }: { status: string }) {
  const colors: Record<string, string> = {
    connected: "bg-green-500",
    connecting: "bg-yellow-500",
    reconnecting: "bg-yellow-500 animate-pulse",
    disconnected: "bg-red-500",
  };
  return (
    <span className="flex items-center gap-1.5 text-xs text-text-muted">
      <span className={cn("w-2 h-2 rounded-full", colors[status] ?? "bg-surface-border")} />
      {status}
    </span>
  );
}

function UserAvatar({ user, size = "md" }: { user?: User; size?: "sm" | "md" }) {
  const name = user?.display_name ?? user?.username ?? "";
  const sz = size === "sm" ? "w-9 h-9 text-xs" : "w-10 h-10 text-sm";
  return (
    <div
      className={cn("rounded-full flex items-center justify-center font-semibold text-white shrink-0", sz)}
      style={{ backgroundColor: hashColor(user?.id ?? name) }}
    >
      {user?.avatar_url
        ? <img src={user.avatar_url} alt={name} className="w-full h-full rounded-full object-cover" />
        : initials(name)
      }
    </div>
  );
}

function SendIcon() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} className="w-4 h-4">
      <line x1="22" y1="2" x2="11" y2="13" />
      <polygon points="22 2 15 22 11 13 2 9 22 2" />
    </svg>
  );
}
