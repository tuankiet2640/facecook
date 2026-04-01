import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { useMutation } from "@tanstack/react-query";
import { authApi } from "@/lib/api";
import { useAuthStore } from "@/stores/auth";
import { cn } from "@/lib/utils";

export function AuthPage() {
  const [tab, setTab] = useState<"login" | "register">("login");
  const navigate = useNavigate();
  const setAuth = useAuthStore((s) => s.setAuth);

  return (
    <div className="min-h-screen bg-surface flex items-center justify-center p-4">
      <div className="w-full max-w-sm">
        {/* Logo */}
        <div className="text-center mb-8">
          <h1 className="text-3xl font-bold text-text-primary">Facecook</h1>
          <p className="text-text-secondary text-sm mt-1">Connect · Share · Chat</p>
        </div>

        {/* Tab switcher */}
        <div className="flex bg-surface-overlay rounded-lg p-1 mb-6">
          {(["login", "register"] as const).map((t) => (
            <button
              key={t}
              onClick={() => setTab(t)}
              className={cn(
                "flex-1 py-2 text-sm font-medium rounded-md transition-all",
                tab === t
                  ? "bg-accent text-white"
                  : "text-text-secondary hover:text-text-primary",
              )}
            >
              {t === "login" ? "Sign in" : "Create account"}
            </button>
          ))}
        </div>

        {tab === "login" ? (
          <LoginForm onSuccess={(token, userId) => { setAuth(token, userId); navigate("/feed"); }} />
        ) : (
          <RegisterForm onSuccess={(token, userId) => { setAuth(token, userId); navigate("/feed"); }} />
        )}
      </div>
    </div>
  );
}

// ── Login ─────────────────────────────────────────────────────────────────────

function LoginForm({ onSuccess }: { onSuccess: (token: string, userId: string) => void }) {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");

  const { mutate, isPending, error } = useMutation({
    mutationFn: () => authApi.login({ username, password }),
    onSuccess: (data) => onSuccess(data.access_token, data.user_id),
  });

  return (
    <form onSubmit={(e) => { e.preventDefault(); mutate(); }} className="space-y-4">
      <Field label="Username">
        <input
          value={username}
          onChange={(e) => setUsername(e.target.value)}
          placeholder="alice"
          autoComplete="username"
          required
          className={inputCls}
        />
      </Field>
      <Field label="Password">
        <input
          type="password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          placeholder="••••••••"
          autoComplete="current-password"
          required
          className={inputCls}
        />
      </Field>
      {error && <p className="text-red-400 text-sm">{getErrorMessage(error)}</p>}
      <button type="submit" disabled={isPending} className={submitCls}>
        {isPending ? "Signing in…" : "Sign in"}
      </button>
    </form>
  );
}

// ── Register ──────────────────────────────────────────────────────────────────

function RegisterForm({ onSuccess }: { onSuccess: (token: string, userId: string) => void }) {
  const [form, setForm] = useState({ username: "", email: "", password: "", display_name: "" });

  const { mutate, isPending, error } = useMutation({
    mutationFn: () => authApi.register(form),
    onSuccess: (data) => onSuccess(data.access_token, data.user_id),
  });

  const set = (k: keyof typeof form) => (e: React.ChangeEvent<HTMLInputElement>) =>
    setForm((f) => ({ ...f, [k]: e.target.value }));

  return (
    <form onSubmit={(e) => { e.preventDefault(); mutate(); }} className="space-y-4">
      <Field label="Username">
        <input value={form.username} onChange={set("username")} placeholder="alice" required className={inputCls} />
      </Field>
      <Field label="Display name">
        <input value={form.display_name} onChange={set("display_name")} placeholder="Alice Smith" className={inputCls} />
      </Field>
      <Field label="Email">
        <input type="email" value={form.email} onChange={set("email")} placeholder="alice@example.com" required className={inputCls} />
      </Field>
      <Field label="Password">
        <input type="password" value={form.password} onChange={set("password")} placeholder="••••••••" required className={inputCls} />
      </Field>
      {error && <p className="text-red-400 text-sm">{getErrorMessage(error)}</p>}
      <button type="submit" disabled={isPending} className={submitCls}>
        {isPending ? "Creating account…" : "Create account"}
      </button>
    </form>
  );
}

// ── Helpers ───────────────────────────────────────────────────────────────────

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="space-y-1.5">
      <label className="text-sm text-text-secondary">{label}</label>
      {children}
    </div>
  );
}

function getErrorMessage(err: unknown): string {
  if (err && typeof err === "object" && "response" in err) {
    const r = (err as { response?: { data?: { error?: { message?: string } } } }).response;
    return r?.data?.error?.message ?? "Something went wrong";
  }
  return "Something went wrong";
}

const inputCls =
  "w-full bg-surface-overlay border border-surface-border rounded-lg px-3 py-2.5 text-sm text-text-primary placeholder:text-text-muted focus:outline-none focus:border-accent transition-colors";

const submitCls =
  "w-full bg-accent hover:bg-accent-hover disabled:opacity-50 text-white font-medium py-2.5 rounded-lg text-sm transition-colors";
