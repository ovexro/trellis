import { ReactNode } from "react";
import { useLocation } from "react-router-dom";
import Sidebar from "./Sidebar";
import Header from "./Header";

interface ShellProps {
  children: ReactNode;
}

export default function Shell({ children }: ShellProps) {
  const location = useLocation();

  return (
    <div className="flex h-screen overflow-hidden">
      <Sidebar />
      <div className="flex flex-col flex-1 overflow-hidden">
        <Header />
        <main
          key={location.pathname}
          className="flex-1 overflow-auto px-8 py-6 animate-page-enter"
        >
          {children}
        </main>
      </div>
    </div>
  );
}
