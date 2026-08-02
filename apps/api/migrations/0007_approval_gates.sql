ALTER TABLE workflow_executions DROP CONSTRAINT workflow_executions_status_check;
ALTER TABLE workflow_executions
    ADD CONSTRAINT workflow_executions_status_check
    CHECK (status IN ('pending', 'running', 'waiting', 'succeeded', 'failed'));

ALTER TABLE workflow_execution_nodes DROP CONSTRAINT workflow_execution_nodes_status_check;
ALTER TABLE workflow_execution_nodes
    ADD CONSTRAINT workflow_execution_nodes_status_check
    CHECK (status IN ('succeeded', 'failed', 'skipped', 'waiting'));

-- A resumed run replays/updates the same (execution_id, node_id) rows rather than
-- inserting duplicates, so upserting needs a uniqueness target.
ALTER TABLE workflow_execution_nodes
    ADD CONSTRAINT workflow_execution_nodes_execution_node_unique UNIQUE (execution_id, node_id);

CREATE TABLE approval_gates (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    execution_id UUID NOT NULL REFERENCES workflow_executions(id) ON DELETE CASCADE,
    node_id UUID NOT NULL REFERENCES workflow_nodes(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'approved', 'rejected')),
    decided_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    decided_at TIMESTAMPTZ,
    UNIQUE (execution_id, node_id)
);
