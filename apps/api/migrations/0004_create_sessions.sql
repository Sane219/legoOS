-- Schema only for now: auth is stateless JWT with no server-side session lookup yet.
-- This exists so a future refresh-token / session-revocation feature (Phase 2+) has a
-- table to land in without another migration.
CREATE TABLE sessions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL
);
