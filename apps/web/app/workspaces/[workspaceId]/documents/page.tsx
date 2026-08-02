"use client";

import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import { useEffect, useState, type FormEvent } from "react";
import {
  ApiError,
  createDocument,
  deleteDocument,
  listDocuments,
  type Document,
} from "@/lib/api";
import { getToken } from "@/lib/auth";

function statusClassName(status: string): string {
  if (status === "ready") return "text-emerald-600 dark:text-emerald-400";
  if (status === "failed") return "text-red-600 dark:text-red-400";
  return "animate-pulse text-zinc-500";
}

export default function DocumentsPage() {
  const router = useRouter();
  const params = useParams<{ workspaceId: string }>();
  const [token, setToken] = useState<string | null>(null);
  const [documents, setDocuments] = useState<Document[] | null>(null);
  const [name, setName] = useState("");
  const [content, setContent] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    const currentToken = getToken();
    if (!currentToken) {
      router.push("/login");
      return;
    }

    listDocuments(currentToken, params.workspaceId)
      .then((docs) => {
        setToken(currentToken);
        setDocuments(docs);
      })
      .catch(() => {
        setToken(currentToken);
        setDocuments([]);
      });
  }, [params.workspaceId, router]);

  useEffect(() => {
    if (!token) return;
    const hasPending = documents?.some((d) => d.status === "pending");
    if (!hasPending) return;

    const interval = setInterval(() => {
      listDocuments(token, params.workspaceId)
        .then(setDocuments)
        .catch(() => {});
    }, 2000);

    return () => clearInterval(interval);
  }, [token, params.workspaceId, documents]);

  async function handleCreate(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!token) return;
    setError(null);
    setSubmitting(true);

    try {
      const document = await createDocument(token, params.workspaceId, {
        name,
        content,
      });
      setDocuments((prev) => [...(prev ?? []), document]);
      setName("");
      setContent("");
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "something went wrong");
    } finally {
      setSubmitting(false);
    }
  }

  async function handleDelete(documentId: string) {
    if (!token) return;
    try {
      await deleteDocument(token, params.workspaceId, documentId);
      setDocuments((prev) => (prev ?? []).filter((d) => d.id !== documentId));
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "something went wrong");
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
      <h1 className="text-2xl font-semibold">Documents</h1>

      {documents === null ? (
        <p className="text-sm text-zinc-500">Loading...</p>
      ) : documents.length === 0 ? (
        <p className="text-sm text-zinc-500">No documents yet.</p>
      ) : (
        <ul className="flex flex-col gap-2">
          {documents.map((document) => (
            <li
              key={document.id}
              className="flex flex-col gap-1 rounded-md border border-zinc-200 px-4 py-3 text-sm dark:border-zinc-800"
            >
              <div className="flex items-center justify-between gap-2">
                <div>
                  <p className="font-medium">{document.name}</p>
                  <p className={`text-xs ${statusClassName(document.status)}`}>
                    {document.status}
                  </p>
                </div>
                <button
                  type="button"
                  onClick={() => handleDelete(document.id)}
                  className="shrink-0 rounded-md border border-red-300 px-3 py-1.5 text-red-600 hover:bg-red-50 dark:border-red-800 dark:text-red-400 dark:hover:bg-red-950"
                >
                  Delete
                </button>
              </div>
              {document.status === "failed" && document.error && (
                <p className="text-sm text-red-600 dark:text-red-400">
                  {document.error}
                </p>
              )}
            </li>
          ))}
        </ul>
      )}

      <form onSubmit={handleCreate} className="flex flex-col gap-2">
        <input
          type="text"
          required
          placeholder="Document name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          className="rounded-md border border-zinc-300 px-3 py-2 text-sm dark:border-zinc-700 dark:bg-zinc-900"
        />
        <textarea
          required
          placeholder="Paste document content..."
          value={content}
          onChange={(e) => setContent(e.target.value)}
          rows={8}
          className="rounded-md border border-zinc-300 px-3 py-2 text-sm dark:border-zinc-700 dark:bg-zinc-900"
        />
        <button
          type="submit"
          disabled={submitting}
          className="rounded-md bg-foreground px-4 py-2 text-sm text-background hover:opacity-90 disabled:opacity-50"
        >
          Upload document
        </button>
      </form>
      {error && <p className="text-sm text-red-600 dark:text-red-400">{error}</p>}
    </main>
  );
}
