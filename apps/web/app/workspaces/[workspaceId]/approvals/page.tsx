"use client";

import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import { useEffect, useState } from "react";
import {
  ApiError,
  approveGate,
  listApprovals,
  rejectGate,
  type ApprovalGate,
} from "@/lib/api";
import { getToken } from "@/lib/auth";

export default function ApprovalsPage() {
  const router = useRouter();
  const params = useParams<{ workspaceId: string }>();
  const [token, setToken] = useState<string | null>(null);
  const [gates, setGates] = useState<ApprovalGate[] | null>(null);
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [pendingId, setPendingId] = useState<string | null>(null);

  useEffect(() => {
    const currentToken = getToken();
    if (!currentToken) {
      router.push("/login");
      return;
    }

    listApprovals(currentToken, params.workspaceId)
      .then((gs) => {
        setToken(currentToken);
        setGates(gs);
      })
      .catch(() => {
        setToken(currentToken);
        setGates([]);
      });
  }, [params.workspaceId, router]);

  async function handleDecision(gateId: string, decide: (id: string) => Promise<unknown>) {
    if (!token) return;
    setPendingId(gateId);
    setErrors((prev) => ({ ...prev, [gateId]: "" }));
    try {
      await decide(gateId);
      setGates((prev) => (prev ?? []).filter((g) => g.id !== gateId));
    } catch (err) {
      const message =
        err instanceof ApiError
          ? err.status === 404
            ? "already decided by someone else"
            : err.message
          : "something went wrong";
      setErrors((prev) => ({ ...prev, [gateId]: message }));
    } finally {
      setPendingId(null);
    }
  }

  function handleApprove(gateId: string) {
    if (!token) return;
    return handleDecision(gateId, (id) => approveGate(token, params.workspaceId, id));
  }

  function handleReject(gateId: string) {
    if (!token) return;
    return handleDecision(gateId, (id) => rejectGate(token, params.workspaceId, id));
  }

  return (
    <main className="flex flex-1 flex-col gap-4 px-6 py-12 max-w-2xl mx-auto w-full">
      <Link
        href={`/workspaces/${params.workspaceId}`}
        className="text-sm text-zinc-500 hover:underline"
      >
        &larr; Workflows
      </Link>
      <h1 className="text-2xl font-semibold">Approvals</h1>

      {gates === null ? (
        <p className="text-sm text-zinc-500">Loading...</p>
      ) : gates.length === 0 ? (
        <p className="text-sm text-zinc-500">No pending approvals.</p>
      ) : (
        <ul className="flex flex-col gap-2">
          {gates.map((gate) => (
            <li
              key={gate.id}
              className="flex flex-col gap-2 rounded-md border border-zinc-200 px-4 py-3 text-sm dark:border-zinc-800"
            >
              <div className="flex items-center justify-between gap-2">
                <div>
                  <p className="font-medium">{gate.workflow_name}</p>
                  <p className="text-xs text-zinc-400">
                    {new Date(gate.created_at).toLocaleString()}
                  </p>
                </div>
                <div className="flex shrink-0 gap-2">
                  <button
                    type="button"
                    onClick={() => handleApprove(gate.id)}
                    disabled={pendingId === gate.id}
                    className="rounded-md bg-foreground px-3 py-1.5 text-sm text-background hover:opacity-90 disabled:opacity-50"
                  >
                    Approve
                  </button>
                  <button
                    type="button"
                    onClick={() => handleReject(gate.id)}
                    disabled={pendingId === gate.id}
                    className="rounded-md border border-red-300 px-3 py-1.5 text-red-600 hover:bg-red-50 disabled:opacity-50 dark:border-red-800 dark:text-red-400 dark:hover:bg-red-950"
                  >
                    Reject
                  </button>
                </div>
              </div>

              <pre className="overflow-x-auto rounded-md bg-zinc-50 p-2 text-xs dark:bg-zinc-900">
                {gate.context === null
                  ? "No context."
                  : JSON.stringify(gate.context, null, 2)}
              </pre>

              {errors[gate.id] && (
                <p className="text-sm text-red-600 dark:text-red-400">
                  {errors[gate.id]}
                </p>
              )}
            </li>
          ))}
        </ul>
      )}
    </main>
  );
}
