import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import ApprovalsPage from "./page";
import { ApiError } from "@/lib/api";

const push = vi.fn();
const listApprovals = vi.fn();
const approveGate = vi.fn();
const rejectGate = vi.fn();

const router = { push };
const routeParams = { workspaceId: "workspace-1" };

vi.mock("next/navigation", () => ({
  useRouter: () => router,
  useParams: () => routeParams,
}));

vi.mock("@/lib/api", async () => {
  const actual = await vi.importActual<typeof import("@/lib/api")>("@/lib/api");
  return {
    ...actual,
    listApprovals: (...args: unknown[]) => listApprovals(...args),
    approveGate: (...args: unknown[]) => approveGate(...args),
    rejectGate: (...args: unknown[]) => rejectGate(...args),
  };
});

const gate = {
  id: "gate-1",
  execution_id: "exec-1",
  workflow_id: "wf-1",
  workflow_name: "My Workflow",
  node_id: "node-1",
  context: { amount: 100 },
  status: "pending",
  created_at: "2026-01-01T00:00:00Z",
};

afterEach(() => {
  window.localStorage.clear();
  vi.clearAllMocks();
});

describe("ApprovalsPage", () => {
  it("renders pending gates with their context", async () => {
    window.localStorage.setItem("legoos_token", "test-token");
    listApprovals.mockResolvedValueOnce([gate]);

    render(<ApprovalsPage />);

    expect(await screen.findByText("My Workflow")).toBeInTheDocument();
    expect(screen.getByText(/"amount": 100/)).toBeInTheDocument();
  });

  it("shows an empty state when there are no pending approvals", async () => {
    window.localStorage.setItem("legoos_token", "test-token");
    listApprovals.mockResolvedValueOnce([]);

    render(<ApprovalsPage />);

    expect(await screen.findByText("No pending approvals.")).toBeInTheDocument();
  });

  it("calls the approve API and removes the gate from the list", async () => {
    window.localStorage.setItem("legoos_token", "test-token");
    listApprovals.mockResolvedValueOnce([gate]);
    approveGate.mockResolvedValueOnce({ status: "approved" });

    render(<ApprovalsPage />);

    await screen.findByText("My Workflow");
    fireEvent.click(screen.getByRole("button", { name: "Approve" }));

    await waitFor(() =>
      expect(approveGate).toHaveBeenCalledWith("test-token", "workspace-1", "gate-1"),
    );
    await waitFor(() =>
      expect(screen.queryByText("My Workflow")).not.toBeInTheDocument(),
    );
  });

  it("calls the reject API and removes the gate from the list", async () => {
    window.localStorage.setItem("legoos_token", "test-token");
    listApprovals.mockResolvedValueOnce([gate]);
    rejectGate.mockResolvedValueOnce({ status: "rejected" });

    render(<ApprovalsPage />);

    await screen.findByText("My Workflow");
    fireEvent.click(screen.getByRole("button", { name: "Reject" }));

    await waitFor(() =>
      expect(rejectGate).toHaveBeenCalledWith("test-token", "workspace-1", "gate-1"),
    );
    await waitFor(() =>
      expect(screen.queryByText("My Workflow")).not.toBeInTheDocument(),
    );
  });

  it("shows an error message when the approve API call fails", async () => {
    window.localStorage.setItem("legoos_token", "test-token");
    listApprovals.mockResolvedValueOnce([gate]);
    approveGate.mockRejectedValueOnce(
      new ApiError("pending approval gate not found", 404),
    );

    render(<ApprovalsPage />);

    await screen.findByText("My Workflow");
    fireEvent.click(screen.getByRole("button", { name: "Approve" }));

    expect(
      await screen.findByText("already decided by someone else"),
    ).toBeInTheDocument();
    expect(screen.getByText("My Workflow")).toBeInTheDocument();
  });
});
