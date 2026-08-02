"use client";

import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import { useCallback, useEffect, useState } from "react";
import { WorkflowCanvas } from "@/components/WorkflowCanvas";
import {
  getWorkflow,
  openExecutionTrace,
  runWorkflow,
  saveGraph,
  type WorkflowGraph,
} from "@/lib/api";
import { getToken } from "@/lib/auth";
import { toBackendGraph, type CanvasEdge, type CanvasNode } from "@/lib/workflow-graph";

export default function WorkflowCanvasPage() {
  const router = useRouter();
  const params = useParams<{ workspaceId: string; workflowId: string }>();
  const [token, setToken] = useState<string | null>(null);
  const [graph, setGraph] = useState<WorkflowGraph | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const currentToken = getToken();
    if (!currentToken) {
      router.push("/login");
      return;
    }

    getWorkflow(currentToken, params.workspaceId, params.workflowId)
      .then((g) => {
        setToken(currentToken);
        setGraph(g);
      })
      .catch(() => setError("could not load this workflow"));
  }, [params.workspaceId, params.workflowId, router]);

  const handleSave = useCallback(
    async (nodes: CanvasNode[], edges: CanvasEdge[]) => {
      if (!token) return;
      const { nodes: backendNodes, edges: backendEdges } = toBackendGraph(nodes, edges);
      const updated = await saveGraph(
        token,
        params.workspaceId,
        params.workflowId,
        backendNodes,
        backendEdges,
      );
      setGraph(updated);
    },
    [token, params.workspaceId, params.workflowId],
  );

  const handleRun = useCallback(async () => {
    if (!token) throw new Error("not authenticated");
    return runWorkflow(token, params.workspaceId, params.workflowId);
  }, [token, params.workspaceId, params.workflowId]);

  const handleSubscribeTrace = useCallback(
    (executionId: string) => {
      if (!token) throw new Error("not authenticated");
      return openExecutionTrace(
        token,
        params.workspaceId,
        params.workflowId,
        executionId,
      );
    },
    [token, params.workspaceId, params.workflowId],
  );

  if (error) {
    return (
      <main className="flex flex-1 items-center justify-center px-6">
        <p className="text-sm text-red-600 dark:text-red-400">{error}</p>
      </main>
    );
  }

  if (!graph) {
    return (
      <main className="flex flex-1 items-center justify-center px-6">
        <p className="text-sm text-zinc-500">Loading...</p>
      </main>
    );
  }

  return (
    <div className="flex flex-1 flex-col">
      <div className="flex items-center gap-3 border-b border-zinc-200 px-4 py-2 dark:border-zinc-800">
        <Link
          href={`/workspaces/${params.workspaceId}`}
          className="text-sm text-zinc-500 hover:underline"
        >
          &larr; Workflows
        </Link>
        <h1 className="text-sm font-semibold">{graph.name}</h1>
        <Link
          href={`/workspaces/${params.workspaceId}/workflows/${params.workflowId}/schedules`}
          className="text-sm text-zinc-500 hover:underline"
        >
          Schedules
        </Link>
      </div>
      <WorkflowCanvas
        graph={graph}
        onSave={handleSave}
        onRun={handleRun}
        onSubscribeTrace={handleSubscribeTrace}
      />
    </div>
  );
}
