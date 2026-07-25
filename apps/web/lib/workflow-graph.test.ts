import { describe, expect, it } from "vitest";
import { toBackendGraph, toCanvasEdges, toCanvasNodes } from "./workflow-graph";
import type { WorkflowEdge, WorkflowNode } from "@/lib/api";

describe("workflow-graph conversions", () => {
  const nodes: WorkflowNode[] = [
    { id: "a", node_type: "input", config: { value: 1 }, position_x: 10, position_y: 20 },
    { id: "b", node_type: "transform", config: { merge: {} }, position_x: 30, position_y: 40 },
  ];
  const edges: WorkflowEdge[] = [
    { id: "e1", source_node_id: "a", target_node_id: "b", condition: null },
  ];

  it("converts backend nodes into positioned canvas nodes carrying node_type and config", () => {
    const canvasNodes = toCanvasNodes(nodes);

    expect(canvasNodes).toEqual([
      {
        id: "a",
        type: "default",
        position: { x: 10, y: 20 },
        data: { label: "input", nodeType: "input", config: { value: 1 } },
      },
      {
        id: "b",
        type: "default",
        position: { x: 30, y: 40 },
        data: { label: "transform", nodeType: "transform", config: { merge: {} } },
      },
    ]);
  });

  it("converts backend edges into canvas edges, labeling conditional edges", () => {
    const conditional: WorkflowEdge[] = [
      { id: "e2", source_node_id: "a", target_node_id: "b", condition: "true" },
    ];

    const [unconditional] = toCanvasEdges(edges);
    const [labeled] = toCanvasEdges(conditional);

    expect(unconditional).toEqual({
      id: "e1",
      source: "a",
      target: "b",
      label: undefined,
      data: { condition: null },
    });
    expect(labeled.label).toBe("true");
    expect(labeled.data).toEqual({ condition: "true" });
  });

  it("round-trips backend -> canvas -> backend without losing data", () => {
    const canvasNodes = toCanvasNodes(nodes);
    const canvasEdges = toCanvasEdges(edges);

    const result = toBackendGraph(canvasNodes, canvasEdges);

    expect(result.nodes).toEqual(nodes);
    expect(result.edges).toEqual(edges);
  });

  it("defaults a canvas edge with no condition data back to null", () => {
    const bareEdge = { id: "e3", source: "a", target: "b" };
    const result = toBackendGraph([], [bareEdge]);

    expect(result.edges).toEqual([
      { id: "e3", source_node_id: "a", target_node_id: "b", condition: null },
    ]);
  });
});
