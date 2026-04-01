import { NavLink, useNavigate } from "react-router-dom";
import { useAuthStore } from "@/stores/auth";
import { cn, initials, hashColor } from "@/lib/utils";

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
      <div className="px-3 mb-6">
        <span className="text-lg font-bold text-text-primary tracking-tight">Facecook</span>
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

// ── Icons ─────────────────────────────────────────────────────────────────────

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
