"use client";

import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import { useEffect, useState } from "react";
import { getWorkflowAnalytics, type ExecutionAnalytics } from "@/lib/api";
import { getToken } from "@/lib/auth";

const STATUS_CLASSES: Record<string, string> = {
  succeeded: "text-green-600 dark:text-green-400",
  failed: "text-red-600 dark:text-red-400",
  running: "text-blue-600 dark:text-blue-400",
  pending: "text-zinc-500",
  waiting: "text-zinc-500",
};

export default function AnalyticsPage() {
  const router = useRouter();
  const params = useParams<{ workspaceId: string; workflowId: string }>();
  const [executions, setExecutions] = useState<ExecutionAnalytics[] | null>(null);

  useEffect(() => {
    const currentToken = getToken();
    if (!currentToken) {
      router.push("/login");
      return;
    }

    getWorkflowAnalytics(currentToken, params.workspaceId, params.workflowId)
      .then(setExecutions)
      .catch(() => setExecutions([]));
  }, [params.workspaceId, params.workflowId, router]);

  const totalCost = (executions ?? []).reduce((sum, e) => sum + e.total_cost_usd, 0);
  const totalTokens = (executions ?? []).reduce(
    (sum, e) => sum + e.total_input_tokens + e.total_output_tokens,
    0,
  );
  const scored = (executions ?? []).filter(
    (e): e is ExecutionAnalytics & { avg_eval_score: number } => e.avg_eval_score !== null,
  );
  const avgScore =
    scored.length > 0
      ? scored.reduce((sum, e) => sum + e.avg_eval_score, 0) / scored.length
      : null;

  return (
    <main className="flex flex-1 flex-col gap-4 px-6 py-12 max-w-3xl mx-auto w-full">
      <Link
        href={`/workspaces/${params.workspaceId}/workflows/${params.workflowId}`}
        className="text-sm text-zinc-500 hover:underline"
      >
        &larr; Workflow
      </Link>
      <h1 className="text-2xl font-semibold">Analytics</h1>

      {executions === null ? (
        <p className="text-sm text-zinc-500">Loading...</p>
      ) : executions.length === 0 ? (
        <p className="text-sm text-zinc-500">No executions yet.</p>
      ) : (
        <>
          <div className="flex gap-6 rounded-md border border-zinc-200 px-4 py-3 text-sm dark:border-zinc-800">
            <div>
              <p className="text-xs text-zinc-500">Total cost</p>
              <p className="font-mono">${totalCost.toFixed(4)}</p>
            </div>
            <div>
              <p className="text-xs text-zinc-500">Total tokens</p>
              <p className="font-mono">{totalTokens}</p>
            </div>
            <div>
              <p className="text-xs text-zinc-500">Avg eval score</p>
              <p className="font-mono">{avgScore !== null ? avgScore.toFixed(2) : "—"}</p>
            </div>
          </div>

          <div className="overflow-x-auto">
            <table className="w-full text-left text-sm">
              <thead>
                <tr className="border-b border-zinc-200 text-xs text-zinc-500 dark:border-zinc-800">
                  <th className="py-2 pr-4">Started</th>
                  <th className="py-2 pr-4">Status</th>
                  <th className="py-2 pr-4">Cost</th>
                  <th className="py-2 pr-4">Input tokens</th>
                  <th className="py-2 pr-4">Output tokens</th>
                  <th className="py-2 pr-4">Eval score</th>
                </tr>
              </thead>
              <tbody>
                {executions.map((execution) => (
                  <tr
                    key={execution.execution_id}
                    className="border-b border-zinc-100 dark:border-zinc-900"
                  >
                    <td className="py-2 pr-4">
                      {new Date(execution.started_at).toLocaleString()}
                    </td>
                    <td
                      className={`py-2 pr-4 ${STATUS_CLASSES[execution.status] ?? "text-zinc-500"}`}
                    >
                      {execution.status}
                    </td>
                    <td className="py-2 pr-4 font-mono">
                      ${execution.total_cost_usd.toFixed(4)}
                    </td>
                    <td className="py-2 pr-4 font-mono">{execution.total_input_tokens}</td>
                    <td className="py-2 pr-4 font-mono">{execution.total_output_tokens}</td>
                    <td className="py-2 pr-4 font-mono">
                      {execution.avg_eval_score !== null
                        ? execution.avg_eval_score.toFixed(2)
                        : "—"}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </>
      )}
    </main>
  );
}
