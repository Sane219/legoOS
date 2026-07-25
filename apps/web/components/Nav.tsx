"use client";

import Link from "next/link";
import { useRouter } from "next/navigation";
import { clearToken, useAuthToken } from "@/lib/auth";

export function Nav() {
  const router = useRouter();
  const authed = useAuthToken() !== null;

  function handleLogout() {
    clearToken();
    router.push("/login");
  }

  return (
    <nav className="flex items-center justify-between border-b border-zinc-200 px-6 py-4 dark:border-zinc-800">
      <Link href="/" className="font-semibold tracking-tight">
        legoOS
      </Link>
      <div className="flex items-center gap-4 text-sm">
        {authed ? (
          <>
            <Link href="/dashboard" className="hover:underline">
              Dashboard
            </Link>
            <button
              type="button"
              onClick={handleLogout}
              className="rounded-md border border-zinc-300 px-3 py-1.5 hover:bg-zinc-50 dark:border-zinc-700 dark:hover:bg-zinc-900"
            >
              Log out
            </button>
          </>
        ) : (
          <>
            <Link href="/login" className="hover:underline">
              Log in
            </Link>
            <Link
              href="/register"
              className="rounded-md bg-foreground px-3 py-1.5 text-background hover:opacity-90"
            >
              Sign up
            </Link>
          </>
        )}
      </div>
    </nav>
  );
}
