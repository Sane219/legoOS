"use client";

import { useRouter } from "next/navigation";
import { useEffect, useState } from "react";
import { WorkspaceList } from "@/components/WorkspaceList";
import { me, type UserResponse } from "@/lib/api";
import { getToken } from "@/lib/auth";

export default function DashboardPage() {
  const router = useRouter();
  const [user, setUser] = useState<UserResponse | null>(null);
  const [token, setToken] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const currentToken = getToken();
    if (!currentToken) {
      router.push("/login");
      return;
    }

    me(currentToken)
      .then((u) => {
        setUser(u);
        setToken(currentToken);
      })
      .catch(() => {
        setError("your session has expired, please log in again");
        router.push("/login");
      });
  }, [router]);

  if (error) {
    return (
      <main className="flex flex-1 items-center justify-center px-6">
        <p className="text-sm text-red-600 dark:text-red-400">{error}</p>
      </main>
    );
  }

  if (!user || !token) {
    return (
      <main className="flex flex-1 items-center justify-center px-6">
        <p className="text-sm text-zinc-500">Loading...</p>
      </main>
    );
  }

  return (
    <main className="flex flex-1 flex-col gap-8 px-6 py-12 max-w-2xl mx-auto w-full">
      <div className="flex flex-col gap-4">
        <h1 className="text-2xl font-semibold">Dashboard</h1>
        <div className="rounded-md border border-zinc-200 p-4 text-sm dark:border-zinc-800">
          <p>
            <span className="text-zinc-500">Email:</span> {user.email}
          </p>
          <p>
            <span className="text-zinc-500">User ID:</span> {user.id}
          </p>
          <p>
            <span className="text-zinc-500">Joined:</span>{" "}
            {new Date(user.created_at).toLocaleString()}
          </p>
        </div>
      </div>

      <WorkspaceList token={token} />
    </main>
  );
}
