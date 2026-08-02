import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { WorkflowCanvas } from "./WorkflowCanvas";
import type { ExecutionResult, ExecutionTraceEvent, WorkflowGraph } from "@/lib/api";

class FakeTraceSocket {
  onmessage: ((event: MessageEvent<string>) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  close = vi.fn(() => this.onclose?.());

  emit(event: ExecutionTraceEvent) {
    this.onmessage?.({ data: JSON.stringify(event) } as MessageEvent<string>);
  }
}

beforeAll(() => {
  // @xyflow/react measures nodes via ResizeObserver, which jsdom doesn't implement.
  global.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver;
});

const graph: WorkflowGraph = {
  id: "wf-1",
  name: "Test workflow",
  created_at: "",
  updated_at: "",
  nodes: [
    { id: "node-1", node_type: "input", config: {}, position_x: 0, position_y: 0 },
  ],
  edges: [],
};

const pendingExecution: ExecutionResult = {
  id: "exec-1",
  workflow_id: "wf-1",
  status: "pending",
  started_at: "",
  finished_at: null,
  nodes: [],
};

describe("WorkflowCanvas", () => {
  afterEach(() => vi.clearAllMocks());

  it("streams live trace events into the execution panel and closes the socket on final", async () => {
    const socket = new FakeTraceSocket();
    const onRun = vi.fn().mockResolvedValue(pendingExecution);
    const onSubscribeTrace = vi.fn().mockReturnValue(socket);

    render(
      <WorkflowCanvas
        graph={graph}
        onSave={vi.fn()}
        onRun={onRun}
        onSubscribeTrace={onSubscribeTrace}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    await waitFor(() => expect(onSubscribeTrace).toHaveBeenCalledWith("exec-1"));
    await screen.findByText("Execution: running");

    socket.emit({
      type: "node_result",
      node_id: "node-1",
      status: "succeeded",
      output: { x: 1 },
      error: null,
    });
    await screen.findByText("succeeded");

    socket.emit({ type: "final", status: "succeeded" });

    await screen.findByText("Execution: succeeded");
    expect(socket.close).toHaveBeenCalled();
  });
});
