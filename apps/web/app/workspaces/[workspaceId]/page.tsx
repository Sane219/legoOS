"use client";

import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import { useEffect, useState, type FormEvent } from "react";
import {
  ApiError,
  createWorkflow,
  listWorkflows,
  type WorkflowResponse,
} from "@/lib/api";
import { getToken } from "@/lib/auth";

export default function WorkspacePage() {
  const router = useRouter();
  const params = useParams<{ workspaceId: string }>();
  const [token, setToken] = useState<string | null>(null);
  const [workflows, setWorkflows] = useState<WorkflowResponse[] | null>(null);
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    const currentToken = getToken();
    if (!currentToken) {
      router.push("/login");
      return;
    }

    listWorkflows(currentToken, params.workspaceId)
      .then((flows) => {
        setToken(currentToken);
        setWorkflows(flows);
      })
      .catch(() => {
        setToken(currentToken);
        setWorkflows([]);
      });
  }, [params.workspaceId, router]);

  async function handleCreate(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!token) return;
    setError(null);
    setSubmitting(true);

    try {
      const workflow = await createWorkflow(token, params.workspaceId, name);
      setWorkflows((prev) => [...(prev ?? []), workflow]);
      setName("");
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "something went wrong");
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <main className="flex flex-1 flex-col gap-4 px-6 py-12 max-w-2xl mx-auto w-full">
      <Link href="/dashboard" className="text-sm text-zinc-500 hover:underline">
        &larr; Dashboard
      </Link>
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold">Workflows</h1>
        <div className="flex gap-4">
          <Link
            href={`/workspaces/${params.workspaceId}/approvals`}
            className="text-sm text-zinc-500 hover:underline"
          >
            Approvals &rarr;
          </Link>
          <Link
            href={`/workspaces/${params.workspaceId}/mcp-connections`}
            className="text-sm text-zinc-500 hover:underline"
          >
            MCP connections &rarr;
          </Link>
          <Link
            href={`/workspaces/${params.workspaceId}/documents`}
            className="text-sm text-zinc-500 hover:underline"
          >
            Documents &rarr;
          </Link>
        </div>
      </div>

      {workflows === null ? (
        <p className="text-sm text-zinc-500">Loading...</p>
      ) : workflows.length === 0 ? (
        <p className="text-sm text-zinc-500">No workflows yet.</p>
      ) : (
        <ul className="flex flex-col gap-2">
          {workflows.map((workflow) => (
            <li key={workflow.id}>
              <Link
                href={`/workspaces/${params.workspaceId}/workflows/${workflow.id}`}
                className="block rounded-md border border-zinc-200 px-4 py-2 text-sm hover:bg-zinc-50 dark:border-zinc-800 dark:hover:bg-zinc-900"
              >
                {workflow.name}
              </Link>
            </li>
          ))}
        </ul>
      )}

      <form onSubmit={handleCreate} className="flex gap-2">
        <input
          type="text"
          required
          placeholder="New workflow name"
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
    </main>
  );
}
