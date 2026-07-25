"use client";

import Link from "next/link";
import { useEffect, useState, type FormEvent } from "react";
import {
  ApiError,
  createWorkspace,
  listWorkspaces,
  type WorkspaceResponse,
} from "@/lib/api";

export function WorkspaceList({ token }: { token: string }) {
  const [workspaces, setWorkspaces] = useState<WorkspaceResponse[] | null>(null);
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    listWorkspaces(token).then(setWorkspaces).catch(() => setWorkspaces([]));
  }, [token]);

  async function handleCreate(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setError(null);
    setSubmitting(true);

    try {
      const workspace = await createWorkspace(token, name);
      setWorkspaces((prev) => [...(prev ?? []), workspace]);
      setName("");
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "something went wrong");
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <div className="flex flex-col gap-4">
      <h2 className="text-lg font-semibold">Workspaces</h2>

      {workspaces === null ? (
        <p className="text-sm text-zinc-500">Loading...</p>
      ) : workspaces.length === 0 ? (
        <p className="text-sm text-zinc-500">No workspaces yet.</p>
      ) : (
        <ul className="flex flex-col gap-2">
          {workspaces.map((workspace) => (
            <li key={workspace.id}>
              <Link
                href={`/workspaces/${workspace.id}`}
                className="flex items-center justify-between rounded-md border border-zinc-200 px-4 py-2 text-sm hover:bg-zinc-50 dark:border-zinc-800 dark:hover:bg-zinc-900"
              >
                <span>{workspace.name}</span>
                <span className="text-zinc-500">{workspace.role}</span>
              </Link>
            </li>
          ))}
        </ul>
      )}

      <form onSubmit={handleCreate} className="flex gap-2">
        <input
          type="text"
          required
          placeholder="New workspace name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          className="flex-1 rounded-md border border-zinc-300 px-3 py-2 text-sm dark:border-zinc-700 dark:bg-zinc-900"
        />
        <button
          type="submit"
          disabled={submitting}
          className="rounded-md bg-foreground px-4 py-2 text-sm text-background hover:opacity-90 disabled:opacity-50"
        >
          Create
        </button>
      </form>
      {error && <p className="text-sm text-red-600 dark:text-red-400">{error}</p>}
    </div>
  );
}
