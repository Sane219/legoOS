CREATE TABLE workflow_schedules (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workflow_id UUID NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
    -- Standard 5-field cron (minute hour day-of-month month day-of-week), UTC.
    cron_expression TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT true,
    next_run_at TIMESTAMPTZ NOT NULL,
    last_run_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- The worker's scheduler tick scans for enabled, due schedules on every pass.
CREATE INDEX workflow_schedules_due_idx ON workflow_schedules (next_run_at) WHERE enabled;
