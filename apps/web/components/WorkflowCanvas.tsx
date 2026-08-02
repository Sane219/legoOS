"use client";

import {
  addEdge,
  Background,
  Controls,
  MiniMap,
  ReactFlow,
  ReactFlowProvider,
  useEdgesState,
  useNodesState,
  type Connection,
  type EdgeMouseHandler,
  type NodeMouseHandler,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type {
  ExecutionResult,
  ExecutionTraceEvent,
  WorkflowGraph,
} from "@/lib/api";
import {
  toCanvasEdges,
  toCanvasNodes,
  type CanvasEdge,
  type CanvasNode,
} from "@/lib/workflow-graph";

const NODE_TYPES = ["input", "transform", "condition"];

interface WorkflowCanvasProps {
  graph: WorkflowGraph;
  onSave: (nodes: CanvasNode[], edges: CanvasEdge[]) => Promise<void>;
  onRun: () => Promise<ExecutionResult>;
  onSubscribeTrace: (executionId: string) => WebSocket;
}

// Spaced wider than a default node (~300px) so newly added nodes don't overlap each other.
function nextPosition(count: number) {
  return {
    x: 80 + (count % 4) * 360,
    y: 80 + Math.floor(count / 4) * 160,
  };
}

function CanvasInner({
  graph,
  onSave,
  onRun,
  onSubscribeTrace,
}: WorkflowCanvasProps) {
  const [nodes, setNodes, onNodesChange] = useNodesState(
    toCanvasNodes(graph.nodes),
  );
  const [edges, setEdges, onEdgesChange] = useEdgesState(
    toCanvasEdges(graph.edges),
  );
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);
  const [selectedEdgeId, setSelectedEdgeId] = useState<string | null>(null);
  const [configText, setConfigText] = useState("");
  const [configError, setConfigError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveMessage, setSaveMessage] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<ExecutionResult | null>(null);
  const traceSocketRef = useRef<WebSocket | null>(null);

  // Close any in-flight trace socket if the user navigates away mid-run.
  useEffect(() => {
    return () => traceSocketRef.current?.close();
  }, []);

  const selectedNode = useMemo(
    () => nodes.find((n) => n.id === selectedNodeId) ?? null,
    [nodes, selectedNodeId],
  );
  const selectedEdge = useMemo(
    () => edges.find((e) => e.id === selectedEdgeId) ?? null,
    [edges, selectedEdgeId],
  );

  const onConnect = useCallback(
    (connection: Connection) => {
      const newEdge: CanvasEdge = { ...connection, id: crypto.randomUUID(), data: { condition: null } };
      setEdges((eds) => addEdge(newEdge, eds));
    },
    [setEdges],
  );

  const onNodeClick: NodeMouseHandler<CanvasNode> = useCallback((_, node) => {
    setSelectedEdgeId(null);
    setSelectedNodeId(node.id);
    setConfigText(JSON.stringify(node.data.config, null, 2));
    setConfigError(null);
  }, []);

  const onEdgeClick: EdgeMouseHandler<CanvasEdge> = useCallback((_, edge) => {
    setSelectedNodeId(null);
    setSelectedEdgeId(edge.id);
  }, []);

  function addNode(nodeType: string) {
    const newNode: CanvasNode = {
      id: crypto.randomUUID(),
      type: "default",
      position: nextPosition(nodes.length),
      data: { label: nodeType, nodeType, config: {} },
    };
    setNodes((nds) => [...nds, newNode]);
  }

  function deleteSelectedNode() {
    if (!selectedNodeId) return;
    setNodes((nds) => nds.filter((n) => n.id !== selectedNodeId));
    setEdges((eds) =>
      eds.filter((e) => e.source !== selectedNodeId && e.target !== selectedNodeId),
    );
    setSelectedNodeId(null);
  }

  function deleteSelectedEdge() {
    if (!selectedEdgeId) return;
    setEdges((eds) => eds.filter((e) => e.id !== selectedEdgeId));
    setSelectedEdgeId(null);
  }

  function applyNodeTypeChange(nodeType: string) {
    if (!selectedNodeId) return;
    setNodes((nds) =>
      nds.map((n) =>
        n.id === selectedNodeId
          ? { ...n, data: { ...n.data, nodeType, label: nodeType } }
          : n,
      ),
    );
  }

  function applyConfigChange() {
    if (!selectedNodeId) return;
    try {
      const parsed: unknown = configText.trim() === "" ? {} : JSON.parse(configText);
      setNodes((nds) =>
        nds.map((n) =>
          n.id === selectedNodeId ? { ...n, data: { ...n.data, config: parsed } } : n,
        ),
      );
      setConfigError(null);
    } catch {
      setConfigError("invalid JSON");
    }
  }

  function applyEdgeCondition(condition: string) {
    if (!selectedEdgeId) return;
    const value = condition.trim() === "" ? null : condition.trim();
    setEdges((eds) =>
      eds.map((e) =>
        e.id === selectedEdgeId ? { ...e, data: { condition: value }, label: value ?? undefined } : e,
      ),
    );
  }

  async function handleSave() {
    setSaving(true);
    setSaveMessage(null);
    try {
      await onSave(nodes, edges);
      setSaveMessage("Saved");
    } catch {
      setSaveMessage("Save failed");
    } finally {
      setSaving(false);
    }
  }

  function applyTraceEvent(event: ExecutionTraceEvent) {
    setResult((current) => {
      if (!current) return current;
      if (event.type === "final") {
        return { ...current, status: event.status };
      }
      const nodeResult = {
        node_id: event.node_id,
        status: event.status,
        output: event.output,
        error: event.error,
      };
      const existingIndex = current.nodes.findIndex(
        (n) => n.node_id === event.node_id,
      );
      const nodes =
        existingIndex === -1
          ? [...current.nodes, nodeResult]
          : current.nodes.map((n, i) => (i === existingIndex ? nodeResult : n));
      return { ...current, nodes };
    });
  }

  async function handleRun() {
    setRunning(true);
    setResult(null);
    traceSocketRef.current?.close();

    try {
      const execution = await onRun();
      setResult({ ...execution, status: "running" });

      await new Promise<void>((resolve) => {
        const socket = onSubscribeTrace(execution.id);
        traceSocketRef.current = socket;

        socket.onmessage = (message: MessageEvent<string>) => {
          const event: ExecutionTraceEvent = JSON.parse(message.data);
          applyTraceEvent(event);
          if (event.type === "final") {
            socket.close();
          }
        };
        socket.onclose = () => resolve();
        socket.onerror = () => resolve();
      });
    } finally {
      traceSocketRef.current = null;
      setRunning(false);
    }
  }

  return (
    <div className="flex flex-1">
      <div className="flex flex-1 flex-col">
        <div className="flex items-center gap-2 border-b border-zinc-200 px-4 py-2 dark:border-zinc-800">
          {NODE_TYPES.map((type) => (
            <button
              key={type}
              type="button"
              onClick={() => addNode(type)}
              className="rounded-md border border-zinc-300 px-3 py-1.5 text-sm hover:bg-zinc-50 dark:border-zinc-700 dark:hover:bg-zinc-900"
            >
              + {type}
            </button>
          ))}
          <div className="flex-1" />
          {saveMessage && <span className="text-sm text-zinc-500">{saveMessage}</span>}
          <button
            type="button"
            onClick={handleSave}
            disabled={saving}
            className="rounded-md border border-zinc-300 px-3 py-1.5 text-sm hover:bg-zinc-50 disabled:opacity-50 dark:border-zinc-700 dark:hover:bg-zinc-900"
          >
            {saving ? "Saving..." : "Save"}
          </button>
          <button
            type="button"
            onClick={handleRun}
            disabled={running}
            className="rounded-md bg-foreground px-3 py-1.5 text-sm text-background hover:opacity-90 disabled:opacity-50"
          >
            {running ? "Running..." : "Run"}
          </button>
        </div>
        <div className="flex-1">
          <ReactFlow
            nodes={nodes}
            edges={edges}
            onNodesChange={onNodesChange}
            onEdgesChange={onEdgesChange}
            onConnect={onConnect}
            onNodeClick={onNodeClick}
            onEdgeClick={onEdgeClick}
            fitView
          >
            <Background />
            <Controls />
            <MiniMap />
          </ReactFlow>
        </div>
      </div>

      <aside className="w-80 shrink-0 overflow-y-auto border-l border-zinc-200 p-4 text-sm dark:border-zinc-800">
        {selectedNode && (
          <div className="flex flex-col gap-3">
            <h3 className="font-semibold">Node</h3>
            <label className="flex flex-col gap-1">
              Type
              <select
                value={selectedNode.data.nodeType}
                onChange={(e) => applyNodeTypeChange(e.target.value)}
                className="rounded-md border border-zinc-300 px-2 py-1 dark:border-zinc-700 dark:bg-zinc-900"
              >
                {NODE_TYPES.map((type) => (
                  <option key={type} value={type}>
                    {type}
                  </option>
                ))}
              </select>
            </label>
            <label className="flex flex-col gap-1">
              Config (JSON)
              <textarea
                value={configText}
                onChange={(e) => setConfigText(e.target.value)}
                onBlur={applyConfigChange}
                rows={8}
                className="rounded-md border border-zinc-300 px-2 py-1 font-mono text-xs dark:border-zinc-700 dark:bg-zinc-900"
              />
            </label>
            {configError && <p className="text-red-600 dark:text-red-400">{configError}</p>}
            <button
              type="button"
              onClick={deleteSelectedNode}
              className="rounded-md border border-red-300 px-3 py-1.5 text-red-600 hover:bg-red-50 dark:border-red-800 dark:text-red-400 dark:hover:bg-red-950"
            >
              Delete node
            </button>
          </div>
        )}

        {selectedEdge && (
          <div className="flex flex-col gap-3">
            <h3 className="font-semibold">Edge</h3>
            <label className="flex flex-col gap-1">
              Condition (blank = always)
              <input
                type="text"
                defaultValue={selectedEdge.data?.condition ?? ""}
                onBlur={(e) => applyEdgeCondition(e.target.value)}
                placeholder="true / false"
                className="rounded-md border border-zinc-300 px-2 py-1 dark:border-zinc-700 dark:bg-zinc-900"
              />
            </label>
            <button
              type="button"
              onClick={deleteSelectedEdge}
              className="rounded-md border border-red-300 px-3 py-1.5 text-red-600 hover:bg-red-50 dark:border-red-800 dark:text-red-400 dark:hover:bg-red-950"
            >
              Delete edge
            </button>
          </div>
        )}

        {!selectedNode && !selectedEdge && (
          <p className="text-zinc-500">Select a node or edge to edit it.</p>
        )}

        {result && (
          <div className="mt-6 flex flex-col gap-2 border-t border-zinc-200 pt-4 dark:border-zinc-800">
            <h3 className="font-semibold">Execution: {result.status}</h3>
            <ul className="flex flex-col gap-1 text-xs">
              {result.nodes.map((n) => (
                <li
                  key={n.node_id}
                  className="rounded border border-zinc-200 p-2 dark:border-zinc-800"
                >
                  <div className="flex justify-between">
                    <span className="font-mono">{n.node_id.slice(0, 8)}</span>
                    <span>{n.status}</span>
                  </div>
                  {n.output != null && (
                    <pre className="mt-1 whitespace-pre-wrap break-all">
                      {JSON.stringify(n.output)}
                    </pre>
                  )}
                  {n.error && (
                    <p className="mt-1 text-red-600 dark:text-red-400">{n.error}</p>
                  )}
                </li>
              ))}
            </ul>
          </div>
        )}
      </aside>
    </div>
  );
}

export function WorkflowCanvas(props: WorkflowCanvasProps) {
  return (
    <ReactFlowProvider>
      <CanvasInner {...props} />
    </ReactFlowProvider>
  );
}
