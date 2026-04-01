import { create } from "zustand";
import type { User } from "@/types/api";

const TOKEN_KEY = "fc_token";
const USER_ID_KEY = "fc_user_id";

interface AuthState {
  token: string | null;
  userId: string | null;
  user: User | null;
  setAuth: (token: string, userId: string) => void;
  setUser: (user: User) => void;
  clear: () => void;
}

export const useAuthStore = create<AuthState>(() => ({
  // Synchronously hydrate from localStorage so there's no flash of login screen
  // on page reload for authenticated users.
  token: localStorage.getItem(TOKEN_KEY),
  userId: localStorage.getItem(USER_ID_KEY),
  user: null,

  setAuth: (token, userId) => {
    localStorage.setItem(TOKEN_KEY, token);
    localStorage.setItem(USER_ID_KEY, userId);
    useAuthStore.setState({ token, userId });
  },

  setUser: (user) => useAuthStore.setState({ user }),

  clear: () => {
    localStorage.removeItem(TOKEN_KEY);
    localStorage.removeItem(USER_ID_KEY);
    useAuthStore.setState({ token: null, userId: null, user: null });
  },
}));
