import { BrowserRouter, Routes, Route, Navigate } from "react-router-dom";
import { AppShell } from "@/components/layout/AppShell";
import { AuthPage } from "@/pages/AuthPage";
import { FeedPage } from "@/pages/FeedPage";
import { ChatPage } from "@/pages/ChatPage";
import { ProfilePage } from "@/pages/ProfilePage";

export default function App() {
  return (
    <BrowserRouter>
      <Routes>
        <Route path="/auth" element={<AuthPage />} />
        <Route element={<AppShell />}>
          <Route path="/feed" element={<FeedPage />} />
          <Route path="/chat" element={<ChatPage />} />
          <Route path="/chat/:convId" element={<ChatPage />} />
          <Route path="/profile/:userId" element={<ProfilePage />} />
          <Route path="/" element={<Navigate to="/feed" replace />} />
          <Route path="*" element={<Navigate to="/feed" replace />} />
        </Route>
      </Routes>
    </BrowserRouter>
  );
}
