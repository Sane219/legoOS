"use client";

import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import { useEffect, useState, type FormEvent } from "react";
import {
  ApiError,
  createSchedule,
  deleteSchedule,
  listSchedules,
  updateSchedule,
  type WorkflowSchedule,
} from "@/lib/api";
import { getToken } from "@/lib/auth";

export default function SchedulesPage() {
  const router = useRouter();
  const params = useParams<{ workspaceId: string; workflowId: string }>();
  const [token, setToken] = useState<string | null>(null);
  const [schedules, setSchedules] = useState<WorkflowSchedule[] | null>(null);
  const [cronExpression, setCronExpression] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [togglingId, setTogglingId] = useState<string | null>(null);

  useEffect(() => {
    const currentToken = getToken();
    if (!currentToken) {
      router.push("/login");
      return;
    }

    listSchedules(currentToken, params.workspaceId, params.workflowId)
      .then((s) => {
        setToken(currentToken);
        setSchedules(s);
      })
      .catch(() => {
        setToken(currentToken);
        setSchedules([]);
      });
  }, [params.workspaceId, params.workflowId, router]);

  async function handleCreate(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!token) return;
    setError(null);
    setSubmitting(true);

    try {
      const schedule = await createSchedule(token, params.workspaceId, params.workflowId, {
        cron_expression: cronExpression,
      });
      setSchedules((prev) => [...(prev ?? []), schedule]);
      setCronExpression("");
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "something went wrong");
    } finally {
      setSubmitting(false);
    }
  }

  async function handleToggle(schedule: WorkflowSchedule) {
    if (!token) return;
    setTogglingId(schedule.id);
    setError(null);
    try {
      const updated = await updateSchedule(
        token,
        params.workspaceId,
        params.workflowId,
        schedule.id,
        { enabled: !schedule.enabled },
      );
      setSchedules((prev) =>
        (prev ?? []).map((s) => (s.id === updated.id ? updated : s)),
      );
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "something went wrong");
    } finally {
      setTogglingId(null);
    }
  }

  async function handleDelete(scheduleId: string) {
    if (!token) return;
    try {
      await deleteSchedule(token, params.workspaceId, params.workflowId, scheduleId);
      setSchedules((prev) => (prev ?? []).filter((s) => s.id !== scheduleId));
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "something went wrong");
    }
  }

  return (
    <main className="flex flex-1 flex-col gap-4 px-6 py-12 max-w-2xl mx-auto w-full">
      <Link
        href={`/workspaces/${params.workspaceId}/workflows/${params.workflowId}`}
        className="text-sm text-zinc-500 hover:underline"
      >
        &larr; Workflow
      </Link>
      <h1 className="text-2xl font-semibold">Schedules</h1>

      {schedules === null ? (
        <p className="text-sm text-zinc-500">Loading...</p>
      ) : schedules.length === 0 ? (
        <p className="text-sm text-zinc-500">No schedules yet.</p>
      ) : (
        <ul className="flex flex-col gap-2">
          {schedules.map((schedule) => (
            <li
              key={schedule.id}
              className="flex items-center justify-between gap-2 rounded-md border border-zinc-200 px-4 py-3 text-sm dark:border-zinc-800"
            >
              <div>
                <p className="font-mono">{schedule.cron_expression}</p>
                <p className="text-xs text-zinc-500">
                  {schedule.enabled ? "Enabled" : "Paused"}
                </p>
                <p className="text-xs text-zinc-400">
                  Next run: {new Date(schedule.next_run_at).toLocaleString()}
                </p>
                <p className="text-xs text-zinc-400">
                  Last run:{" "}
                  {schedule.last_run_at
                    ? new Date(schedule.last_run_at).toLocaleString()
                    : "Never"}
                </p>
              </div>
              <div className="flex shrink-0 gap-2">
                <button
                  type="button"
                  onClick={() => handleToggle(schedule)}
                  disabled={togglingId === schedule.id}
                  className="rounded-md border border-zinc-300 px-3 py-1.5 text-sm hover:bg-zinc-50 disabled:opacity-50 dark:border-zinc-700 dark:hover:bg-zinc-900"
                >
                  {schedule.enabled ? "Pause" : "Resume"}
                </button>
                <button
                  type="button"
                  onClick={() => handleDelete(schedule.id)}
                  className="rounded-md border border-red-300 px-3 py-1.5 text-red-600 hover:bg-red-50 dark:border-red-800 dark:text-red-400 dark:hover:bg-red-950"
                >
                  Delete
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}

      <form onSubmit={handleCreate} className="flex flex-col gap-2">
        <label className="text-sm text-zinc-500" htmlFor="cron-expression">
          Cron expression (UTC, 5 fields: minute hour day month weekday)
        </label>
        <input
          id="cron-expression"
          type="text"
          required
          placeholder="0 9 * * mon-fri"
          value={cronExpression}
          onChange={(e) => setCronExpression(e.target.value)}
          className="rounded-md border border-zinc-300 px-3 py-2 text-sm font-mono dark:border-zinc-700 dark:bg-zinc-900"
        />
        <button
          type="submit"
          disabled={submitting}
          className="rounded-md bg-foreground px-4 py-2 text-sm text-background hover:opacity-90 disabled:opacity-50"
        >
          Add schedule
        </button>
      </form>
      {error && <p className="text-sm text-red-600 dark:text-red-400">{error}</p>}
    </main>
  );
}
