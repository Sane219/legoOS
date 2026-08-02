import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import AnalyticsPage from "./page";

const push = vi.fn();
const getWorkflowAnalytics = vi.fn();

const router = { push };
const routeParams = { workspaceId: "workspace-1", workflowId: "workflow-1" };

vi.mock("next/navigation", () => ({
  useRouter: () => router,
  useParams: () => routeParams,
}));

vi.mock("@/lib/api", async () => {
  const actual = await vi.importActual<typeof import("@/lib/api")>("@/lib/api");
  return {
    ...actual,
    getWorkflowAnalytics: (...args: unknown[]) => getWorkflowAnalytics(...args),
  };
});

const execution = {
  execution_id: "exec-1",
  status: "succeeded",
  started_at: "2026-08-02T10:00:00Z",
  total_cost_usd: 0.0034,
  total_input_tokens: 120,
  total_output_tokens: 45,
  avg_eval_score: 0.9,
};

afterEach(() => {
  window.localStorage.clear();
  vi.clearAllMocks();
});

describe("AnalyticsPage", () => {
  it("shows a loading state before the analytics resolve", async () => {
    window.localStorage.setItem("legoos_token", "test-token");
    getWorkflowAnalytics.mockReturnValueOnce(new Promise(() => {}));

    render(<AnalyticsPage />);

    expect(screen.getByText("Loading...")).toBeInTheDocument();
  });

  it("renders execution rows with cost, token, and score formatting", async () => {
    window.localStorage.setItem("legoos_token", "test-token");
    getWorkflowAnalytics.mockResolvedValueOnce([execution]);

    render(<AnalyticsPage />);

    expect(await screen.findByText("succeeded")).toBeInTheDocument();
    expect(screen.getAllByText("$0.0034").length).toBeGreaterThan(0);
    expect(screen.getByText("120")).toBeInTheDocument();
    expect(screen.getByText("45")).toBeInTheDocument();
    expect(screen.getAllByText("0.90").length).toBeGreaterThan(0);
    expect(
      screen.getByText(new Date(execution.started_at).toLocaleString()),
    ).toBeInTheDocument();
  });

  it("renders a dash for executions with no eval score", async () => {
    window.localStorage.setItem("legoos_token", "test-token");
    getWorkflowAnalytics.mockResolvedValueOnce([
      { ...execution, status: "failed", avg_eval_score: null },
    ]);

    render(<AnalyticsPage />);

    expect(await screen.findByText("failed")).toBeInTheDocument();
    expect(screen.getAllByText("—").length).toBeGreaterThan(0);
  });

  it("shows an empty state when there are no executions", async () => {
    window.localStorage.setItem("legoos_token", "test-token");
    getWorkflowAnalytics.mockResolvedValueOnce([]);

    render(<AnalyticsPage />);

    expect(await screen.findByText("No executions yet.")).toBeInTheDocument();
  });
});
