import { useEffect, useRef, useState } from "react";
import { NavLink, useNavigate } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import { useAuthStore } from "@/stores/auth";
import { usersApi } from "@/lib/api";
import { qk } from "@/lib/queryKeys";
import { cn, initials, hashColor } from "@/lib/utils";
import type { User } from "@/types/api";

const navItems = [
  { to: "/feed", label: "Feed", icon: FeedIcon },
  { to: "/chat", label: "Messages", icon: ChatIcon },
];

export function Sidebar() {
  const user = useAuthStore((s) => s.user);
  const userId = useAuthStore((s) => s.userId);
  const clear = useAuthStore((s) => s.clear);
  const navigate = useNavigate();

  const name = user?.display_name ?? user?.username ?? "";

  return (
    <aside className="w-56 shrink-0 border-r border-surface-border flex flex-col py-4 px-3">
      {/* Logo */}
      <div className="px-3 mb-4">
        <span className="text-lg font-bold text-text-primary tracking-tight">Facecook</span>
      </div>

      {/* Search */}
      <div className="px-1 mb-4">
        <UserSearch />
      </div>

      {/* Nav */}
      <nav className="space-y-1 flex-1">
        {navItems.map(({ to, label, icon: Icon }) => (
          <NavLink
            key={to}
            to={to}
            className={({ isActive }) =>
              cn(
                "flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition-colors",
                isActive
                  ? "bg-accent/10 text-accent"
                  : "text-text-secondary hover:bg-surface-overlay hover:text-text-primary",
              )
            }
          >
            <Icon className="w-5 h-5 shrink-0" />
            {label}
          </NavLink>
        ))}
      </nav>

      {/* Current user */}
      <div className="mt-auto space-y-2">
        <button
          onClick={() => navigate(`/profile/${userId}`)}
          className="w-full flex items-center gap-2.5 px-3 py-2 rounded-lg hover:bg-surface-overlay transition-colors text-left"
        >
          <div
            className="w-7 h-7 rounded-full flex items-center justify-center text-xs font-bold text-white shrink-0"
            style={{ backgroundColor: hashColor(userId ?? name) }}
          >
            {user?.avatar_url
              ? <img src={user.avatar_url} className="w-full h-full rounded-full object-cover" alt={name} />
              : initials(name)
            }
          </div>
          <span className="text-sm text-text-primary truncate font-medium">
            {user?.display_name ?? user?.username ?? "…"}
          </span>
        </button>

        <button
          onClick={() => { clear(); navigate("/auth"); }}
          className="w-full text-left px-3 py-2 text-xs text-text-muted hover:text-red-400 transition-colors"
        >
          Sign out
        </button>
      </div>
    </aside>
  );
}

// ── User search ───────────────────────────────────────────────────────────────
//
// Debounced query against /users/search. Dropdown is only rendered while the
// input is focused AND the debounced query is long enough — clicking outside
// closes it. Each result navigates to /profile/:id.
function UserSearch() {
  const navigate = useNavigate();
  const myId = useAuthStore((s) => s.userId);
  const [raw, setRaw] = useState("");
  const [debounced, setDebounced] = useState("");
  const [open, setOpen] = useState(false);
  const wrapRef = useRef<HTMLDivElement>(null);

  // Debounce: wait 200ms after the user stops typing before firing a query.
  useEffect(() => {
    const t = setTimeout(() => setDebounced(raw.trim()), 200);
    return () => clearTimeout(t);
  }, [raw]);

  // Close dropdown on outside click.
  useEffect(() => {
    function onDocClick(e: MouseEvent) {
      if (!wrapRef.current?.contains(e.target as Node)) setOpen(false);
    }
    document.addEventListener("mousedown", onDocClick);
    return () => document.removeEventListener("mousedown", onDocClick);
  }, []);

  const { data: results, isFetching } = useQuery({
    queryKey: qk.userSearch(debounced),
    queryFn: () => usersApi.search(debounced, 8),
    enabled: debounced.length >= 2,
    staleTime: 30_000,
  });

  // Hide the current user from their own results — redundant to click yourself.
  const filtered: User[] = (results ?? []).filter((u) => u.id !== myId);
  const showDropdown = open && debounced.length >= 2;

  return (
    <div ref={wrapRef} className="relative">
      <div className="relative">
        <SearchIcon className="w-4 h-4 absolute left-2.5 top-1/2 -translate-y-1/2 text-text-muted pointer-events-none" />
        <input
          value={raw}
          onChange={(e) => setRaw(e.target.value)}
          onFocus={() => setOpen(true)}
          placeholder="Search users"
          className="w-full bg-surface-overlay border border-surface-border rounded-lg pl-8 pr-2 py-1.5 text-sm text-text-primary placeholder:text-text-muted focus:outline-none focus:border-accent/50"
        />
      </div>

      {showDropdown && (
        <div className="absolute left-0 right-0 top-full mt-1 bg-surface-raised border border-surface-border rounded-lg shadow-lg overflow-hidden z-20 max-h-80 overflow-y-auto">
          {isFetching && filtered.length === 0 && (
            <p className="px-3 py-2 text-xs text-text-muted">Searching…</p>
          )}
          {!isFetching && filtered.length === 0 && (
            <p className="px-3 py-2 text-xs text-text-muted">No users found.</p>
          )}
          {filtered.map((u) => {
            const name = u.display_name ?? u.username;
            return (
              <button
                key={u.id}
                onClick={() => {
                  setOpen(false);
                  setRaw("");
                  navigate(`/profile/${u.id}`);
                }}
                className="w-full px-3 py-2 flex items-center gap-2.5 text-left hover:bg-surface-overlay transition-colors"
              >
                <div
                  className="w-7 h-7 rounded-full flex items-center justify-center text-[10px] font-bold text-white shrink-0"
                  style={{ backgroundColor: hashColor(u.id) }}
                >
                  {u.avatar_url
                    ? <img src={u.avatar_url} className="w-full h-full rounded-full object-cover" alt={name} />
                    : initials(name)
                  }
                </div>
                <div className="min-w-0">
                  <p className="text-sm font-medium text-text-primary truncate">{name}</p>
                  <p className="text-xs text-text-muted truncate">@{u.username}</p>
                </div>
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}

// ── Icons ─────────────────────────────────────────────────────────────────────

function SearchIcon({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round" className={className}>
      <circle cx="11" cy="11" r="7" />
      <line x1="21" y1="21" x2="16.65" y2="16.65" />
    </svg>
  );
}

function FeedIcon({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.75} className={className}>
      <path d="M3 9l9-7 9 7v11a2 2 0 01-2 2H5a2 2 0 01-2-2z" />
      <polyline points="9 22 9 12 15 12 15 22" />
    </svg>
  );
}

function ChatIcon({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.75} className={className}>
      <path d="M21 15a2 2 0 01-2 2H7l-4 4V5a2 2 0 012-2h14a2 2 0 012 2z" />
    </svg>
  );
}
