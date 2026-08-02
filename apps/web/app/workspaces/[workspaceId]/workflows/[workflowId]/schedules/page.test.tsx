import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import SchedulesPage from "./page";
import { ApiError } from "@/lib/api";

const push = vi.fn();
const listSchedules = vi.fn();
const createSchedule = vi.fn();
const updateSchedule = vi.fn();
const deleteSchedule = vi.fn();

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
    listSchedules: (...args: unknown[]) => listSchedules(...args),
    createSchedule: (...args: unknown[]) => createSchedule(...args),
    updateSchedule: (...args: unknown[]) => updateSchedule(...args),
    deleteSchedule: (...args: unknown[]) => deleteSchedule(...args),
  };
});

const schedule = {
  id: "sched-1",
  workflow_id: "workflow-1",
  cron_expression: "0 9 * * mon-fri",
  enabled: true,
  next_run_at: "2026-01-02T09:00:00Z",
  last_run_at: "2026-01-01T09:00:00Z",
  created_at: "2026-01-01T00:00:00Z",
};

afterEach(() => {
  window.localStorage.clear();
  vi.clearAllMocks();
});

describe("SchedulesPage", () => {
  it("renders the schedule list with next/last run times", async () => {
    window.localStorage.setItem("legoos_token", "test-token");
    listSchedules.mockResolvedValueOnce([schedule]);

    render(<SchedulesPage />);

    expect(await screen.findByText("0 9 * * mon-fri")).toBeInTheDocument();
    expect(screen.getByText("Enabled")).toBeInTheDocument();
    expect(
      screen.getByText(`Next run: ${new Date(schedule.next_run_at).toLocaleString()}`),
    ).toBeInTheDocument();
    expect(
      screen.getByText(`Last run: ${new Date(schedule.last_run_at).toLocaleString()}`),
    ).toBeInTheDocument();
  });

  it("shows Never for a schedule that has not run", async () => {
    window.localStorage.setItem("legoos_token", "test-token");
    listSchedules.mockResolvedValueOnce([{ ...schedule, last_run_at: null }]);

    render(<SchedulesPage />);

    await screen.findByText("0 9 * * mon-fri");
    expect(screen.getByText("Last run: Never")).toBeInTheDocument();
  });

  it("submits the add-schedule form and calls the create API", async () => {
    window.localStorage.setItem("legoos_token", "test-token");
    listSchedules.mockResolvedValueOnce([]);
    createSchedule.mockResolvedValueOnce({
      ...schedule,
      id: "sched-2",
      cron_expression: "0 0 * * *",
    });

    render(<SchedulesPage />);

    await screen.findByText("No schedules yet.");

    fireEvent.change(screen.getByPlaceholderText("0 9 * * mon-fri"), {
      target: { value: "0 0 * * *" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add schedule" }));

    await waitFor(() =>
      expect(createSchedule).toHaveBeenCalledWith("test-token", "workspace-1", "workflow-1", {
        cron_expression: "0 0 * * *",
      }),
    );
    expect(await screen.findByText("0 0 * * *")).toBeInTheDocument();
  });

  it("surfaces a validation error from a rejected create request", async () => {
    window.localStorage.setItem("legoos_token", "test-token");
    listSchedules.mockResolvedValueOnce([]);
    createSchedule.mockRejectedValueOnce(new ApiError("invalid cron expression", 400));

    render(<SchedulesPage />);

    await screen.findByText("No schedules yet.");

    fireEvent.change(screen.getByPlaceholderText("0 9 * * mon-fri"), {
      target: { value: "not a cron" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add schedule" }));

    expect(await screen.findByText("invalid cron expression")).toBeInTheDocument();
  });

  it("calls the update API when the pause/resume button is clicked", async () => {
    window.localStorage.setItem("legoos_token", "test-token");
    listSchedules.mockResolvedValueOnce([schedule]);
    updateSchedule.mockResolvedValueOnce({ ...schedule, enabled: false });

    render(<SchedulesPage />);

    await screen.findByText("0 9 * * mon-fri");
    fireEvent.click(screen.getByRole("button", { name: "Pause" }));

    await waitFor(() =>
      expect(updateSchedule).toHaveBeenCalledWith(
        "test-token",
        "workspace-1",
        "workflow-1",
        "sched-1",
        { enabled: false },
      ),
    );
    expect(await screen.findByText("Paused")).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: "Resume" })).toBeInTheDocument();
  });

  it("calls the delete API when the delete button is clicked", async () => {
    window.localStorage.setItem("legoos_token", "test-token");
    listSchedules.mockResolvedValueOnce([schedule]);
    deleteSchedule.mockResolvedValueOnce({ deleted: true });

    render(<SchedulesPage />);

    await screen.findByText("0 9 * * mon-fri");
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));

    await waitFor(() =>
      expect(deleteSchedule).toHaveBeenCalledWith(
        "test-token",
        "workspace-1",
        "workflow-1",
        "sched-1",
      ),
    );
    await waitFor(() =>
      expect(screen.queryByText("0 9 * * mon-fri")).not.toBeInTheDocument(),
    );
  });
});
