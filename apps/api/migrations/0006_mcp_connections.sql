CREATE TABLE mcp_connections (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    url TEXT NOT NULL,
    -- AES-256-GCM ciphertext (base64(nonce || ciphertext)), see mcp::encrypt_token.
    -- NULL means the server doesn't require a bearer token.
    encrypted_bearer_token TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, name)
);
