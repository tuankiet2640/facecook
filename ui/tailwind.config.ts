import type { Config } from "tailwindcss";

export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        surface: {
          DEFAULT: "#111111",
          raised: "#1a1a1a",
          overlay: "#222222",
          border: "#2a2a2a",
        },
        accent: {
          DEFAULT: "#6366f1",   // indigo-500
          hover: "#4f46e5",     // indigo-600
          muted: "#312e81",     // indigo-900
        },
        text: {
          primary: "#f5f5f5",
          secondary: "#a3a3a3",
          muted: "#525252",
        },
      },
      fontFamily: {
        sans: [
          "Inter",
          "-apple-system",
          "BlinkMacSystemFont",
          "Segoe UI",
          "sans-serif",
        ],
      },
    },
  },
  plugins: [],
} satisfies Config;
