ALTER TABLE workflow_executions DROP CONSTRAINT workflow_executions_status_check;
ALTER TABLE workflow_executions
    ADD CONSTRAINT workflow_executions_status_check
    CHECK (status IN ('pending', 'running', 'succeeded', 'failed'));

ALTER TABLE workflow_executions ALTER COLUMN status SET DEFAULT 'pending';
ALTER TABLE workflow_executions ALTER COLUMN finished_at DROP NOT NULL;
ALTER TABLE workflow_executions ALTER COLUMN finished_at DROP DEFAULT;
