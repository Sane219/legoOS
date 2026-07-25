import type { Edge, Node } from "@xyflow/react";
import type { WorkflowEdge, WorkflowNode } from "@/lib/api";

export interface CanvasNodeData extends Record<string, unknown> {
  label: string;
  nodeType: string;
  config: unknown;
}

export type CanvasNode = Node<CanvasNodeData>;
export type CanvasEdge = Edge<{ condition: string | null }>;

export function toCanvasNodes(nodes: WorkflowNode[]): CanvasNode[] {
  return nodes.map((n) => ({
    id: n.id,
    type: "default",
    position: { x: n.position_x, y: n.position_y },
    data: { label: n.node_type, nodeType: n.node_type, config: n.config },
  }));
}

export function toCanvasEdges(edges: WorkflowEdge[]): CanvasEdge[] {
  return edges.map((e) => ({
    id: e.id,
    source: e.source_node_id,
    target: e.target_node_id,
    label: e.condition ?? undefined,
    data: { condition: e.condition },
  }));
}

export function toBackendGraph(
  nodes: CanvasNode[],
  edges: CanvasEdge[],
): { nodes: WorkflowNode[]; edges: WorkflowEdge[] } {
  return {
    nodes: nodes.map((n) => ({
      id: n.id,
      node_type: n.data.nodeType,
      config: n.data.config,
      position_x: n.position.x,
      position_y: n.position.y,
    })),
    edges: edges.map((e) => ({
      id: e.id,
      source_node_id: e.source,
      target_node_id: e.target,
      condition: e.data?.condition ?? null,
    })),
  };
}
