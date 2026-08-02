"use client";

import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import { useEffect, useState, type FormEvent } from "react";
import {
  ApiError,
  createMcpConnection,
  deleteMcpConnection,
  listMcpConnections,
  testMcpConnection,
  type McpConnection,
  type McpTool,
} from "@/lib/api";
import { getToken } from "@/lib/auth";

export default function McpConnectionsPage() {
  const router = useRouter();
  const params = useParams<{ workspaceId: string }>();
  const [token, setToken] = useState<string | null>(null);
  const [connections, setConnections] = useState<McpConnection[] | null>(null);
  const [name, setName] = useState("");
  const [url, setUrl] = useState("");
  const [bearerToken, setBearerToken] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  // Per-connection test results/errors, keyed by connection id.
  const [testResults, setTestResults] = useState<Record<string, McpTool[]>>({});
  const [testErrors, setTestErrors] = useState<Record<string, string>>({});
  const [testingId, setTestingId] = useState<string | null>(null);

  useEffect(() => {
    const currentToken = getToken();
    if (!currentToken) {
      router.push("/login");
      return;
    }

    listMcpConnections(currentToken, params.workspaceId)
      .then((conns) => {
        setToken(currentToken);
        setConnections(conns);
      })
      .catch(() => {
        setToken(currentToken);
        setConnections([]);
      });
  }, [params.workspaceId, router]);

  async function handleCreate(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!token) return;
    setError(null);
    setSubmitting(true);

    try {
      const connection = await createMcpConnection(token, params.workspaceId, {
        name,
        url,
        ...(bearerToken ? { bearer_token: bearerToken } : {}),
      });
      setConnections((prev) => [...(prev ?? []), connection]);
      setName("");
      setUrl("");
      setBearerToken("");
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "something went wrong");
    } finally {
      setSubmitting(false);
    }
  }

  async function handleDelete(connectionId: string) {
    if (!token) return;
    try {
      await deleteMcpConnection(token, params.workspaceId, connectionId);
      setConnections((prev) => (prev ?? []).filter((c) => c.id !== connectionId));
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "something went wrong");
    }
  }

  async function handleTest(connectionId: string) {
    if (!token) return;
    setTestingId(connectionId);
    setTestErrors((prev) => ({ ...prev, [connectionId]: "" }));
    try {
      const tools = await testMcpConnection(token, params.workspaceId, connectionId);
      setTestResults((prev) => ({ ...prev, [connectionId]: tools }));
    } catch (err) {
      setTestErrors((prev) => ({
        ...prev,
        [connectionId]:
          err instanceof ApiError ? err.message : "something went wrong",
      }));
    } finally {
      setTestingId(null);
    }
  }

  return (
    <main className="flex flex-1 flex-col gap-4 px-6 py-12 max-w-2xl mx-auto w-full">
      <Link
        href={`/workspaces/${params.workspaceId}`}
        className="text-sm text-zinc-500 hover:underline"
      >
        &larr; Workflows
      </Link>
      <h1 className="text-2xl font-semibold">MCP connections</h1>

      {connections === null ? (
        <p className="text-sm text-zinc-500">Loading...</p>
      ) : connections.length === 0 ? (
        <p className="text-sm text-zinc-500">No MCP connections yet.</p>
      ) : (
        <ul className="flex flex-col gap-2">
          {connections.map((connection) => (
            <li
              key={connection.id}
              className="flex flex-col gap-2 rounded-md border border-zinc-200 px-4 py-3 text-sm dark:border-zinc-800"
            >
              <div className="flex items-center justify-between gap-2">
                <div>
                  <p className="font-medium">{connection.name}</p>
                  <p className="text-zinc-500">{connection.url}</p>
                  <p className="text-xs text-zinc-400">
                    {connection.has_token ? "Token saved" : "No token"}
                  </p>
                </div>
                <div className="flex shrink-0 gap-2">
                  <button
                    type="button"
                    onClick={() => handleTest(connection.id)}
                    disabled={testingId === connection.id}
                    className="rounded-md border border-zinc-300 px-3 py-1.5 text-sm hover:bg-zinc-50 disabled:opacity-50 dark:border-zinc-700 dark:hover:bg-zinc-900"
                  >
                    {testingId === connection.id ? "Testing..." : "Test"}
                  </button>
                  <button
                    type="button"
                    onClick={() => handleDelete(connection.id)}
                    className="rounded-md border border-red-300 px-3 py-1.5 text-red-600 hover:bg-red-50 dark:border-red-800 dark:text-red-400 dark:hover:bg-red-950"
                  >
                    Delete
                  </button>
                </div>
              </div>

              {testErrors[connection.id] && (
                <p className="text-sm text-red-600 dark:text-red-400">
                  {testErrors[connection.id]}
                </p>
              )}

              {testResults[connection.id] && (
                <ul className="flex flex-col gap-1 rounded-md bg-zinc-50 p-2 dark:bg-zinc-900">
                  {testResults[connection.id].length === 0 ? (
                    <li className="text-xs text-zinc-500">No tools found.</li>
                  ) : (
                    testResults[connection.id].map((tool) => (
                      <li key={tool.name} className="text-xs">
                        <span className="font-mono font-medium">{tool.name}</span>
                        {tool.description && (
                          <span className="text-zinc-500"> — {tool.description}</span>
                        )}
                      </li>
                    ))
                  )}
                </ul>
              )}
            </li>
          ))}
        </ul>
      )}

      <form onSubmit={handleCreate} className="flex flex-col gap-2">
        <input
          type="text"
          required
          placeholder="Name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          className="rounded-md border border-zinc-300 px-3 py-2 text-sm dark:border-zinc-700 dark:bg-zinc-900"
        />
        <input
          type="url"
          required
          placeholder="https://example.com/mcp"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          className="rounded-md border border-zinc-300 px-3 py-2 text-sm dark:border-zinc-700 dark:bg-zinc-900"
        />
        <input
          type="password"
          placeholder="Bearer token (optional)"
          value={bearerToken}
          onChange={(e) => setBearerToken(e.target.value)}
          className="rounded-md border border-zinc-300 px-3 py-2 text-sm dark:border-zinc-700 dark:bg-zinc-900"
        />
        <button
          type="submit"
          disabled={submitting}
          className="rounded-md bg-foreground px-4 py-2 text-sm text-background hover:opacity-90 disabled:opacity-50"
        >
          Add connection
        </button>
      </form>
      {error && <p className="text-sm text-red-600 dark:text-red-400">{error}</p>}
    </main>
  );
}
