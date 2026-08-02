const API_URL = process.env.NEXT_PUBLIC_API_URL ?? "http://localhost:8080";

export interface AuthResponse {
  token: string;
}

export interface UserResponse {
  id: string;
  email: string;
  created_at: string;
}

export interface WorkspaceResponse {
  id: string;
  name: string;
  created_at: string;
  role: string;
}

export interface WorkflowResponse {
  id: string;
  name: string;
  created_at: string;
  updated_at: string;
}

export interface WorkflowNode {
  id: string;
  node_type: string;
  config: unknown;
  position_x: number;
  position_y: number;
}

export interface WorkflowEdge {
  id: string;
  source_node_id: string;
  target_node_id: string;
  condition: string | null;
}

export interface WorkflowGraph extends WorkflowResponse {
  nodes: WorkflowNode[];
  edges: WorkflowEdge[];
}

export interface ExecutionNodeResult {
  node_id: string;
  status: string;
  output: unknown;
  error: string | null;
}

export interface ExecutionResult {
  id: string;
  workflow_id: string;
  status: string;
  started_at: string;
  finished_at: string | null;
  nodes: ExecutionNodeResult[];
}

export interface ExecutionTraceNodeEvent {
  type: "node_result";
  node_id: string;
  status: string;
  output: unknown;
  error: string | null;
}

export interface ExecutionTraceFinalEvent {
  type: "final";
  status: string;
}

export type ExecutionTraceEvent =
  | ExecutionTraceNodeEvent
  | ExecutionTraceFinalEvent;

export interface McpConnection {
  id: string;
  name: string;
  url: string;
  has_token: boolean;
  created_at: string;
}

export interface McpTool {
  name: string;
  description: string | null;
}

export interface ApprovalGate {
  id: string;
  execution_id: string;
  workflow_id: string;
  workflow_name: string;
  node_id: string;
  context: unknown | null;
  status: string;
  created_at: string;
}

export class ApiError extends Error {
  constructor(
    message: string,
    public status: number,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

async function request<T>(path: string, options: RequestInit = {}): Promise<T> {
  const response = await fetch(`${API_URL}${path}`, {
    ...options,
    headers: {
      "Content-Type": "application/json",
      ...options.headers,
    },
  });

  if (!response.ok) {
    const body = await response.json().catch(() => null);
    const message =
      body && typeof body.error === "string"
        ? body.error
        : `request failed with status ${response.status}`;
    throw new ApiError(message, response.status);
  }

  return response.json() as Promise<T>;
}

function authedRequest<T>(
  path: string,
  token: string,
  options: RequestInit = {},
): Promise<T> {
  return request(path, {
    ...options,
    headers: { Authorization: `Bearer ${token}`, ...options.headers },
  });
}

export function register(
  email: string,
  password: string,
): Promise<AuthResponse> {
  return request("/api/auth/register", {
    method: "POST",
    body: JSON.stringify({ email, password }),
  });
}

export function login(email: string, password: string): Promise<AuthResponse> {
  return request("/api/auth/login", {
    method: "POST",
    body: JSON.stringify({ email, password }),
  });
}

export function me(token: string): Promise<UserResponse> {
  return authedRequest("/api/auth/me", token);
}

export function createWorkspace(
  token: string,
  name: string,
): Promise<WorkspaceResponse> {
  return authedRequest("/api/workspaces", token, {
    method: "POST",
    body: JSON.stringify({ name }),
  });
}

export function listWorkspaces(token: string): Promise<WorkspaceResponse[]> {
  return authedRequest("/api/workspaces", token);
}

export function createWorkflow(
  token: string,
  workspaceId: string,
  name: string,
): Promise<WorkflowResponse> {
  return authedRequest(`/api/workspaces/${workspaceId}/workflows`, token, {
    method: "POST",
    body: JSON.stringify({ name }),
  });
}

export function listWorkflows(
  token: string,
  workspaceId: string,
): Promise<WorkflowResponse[]> {
  return authedRequest(`/api/workspaces/${workspaceId}/workflows`, token);
}

export function getWorkflow(
  token: string,
  workspaceId: string,
  workflowId: string,
): Promise<WorkflowGraph> {
  return authedRequest(
    `/api/workspaces/${workspaceId}/workflows/${workflowId}`,
    token,
  );
}

export function saveGraph(
  token: string,
  workspaceId: string,
  workflowId: string,
  nodes: WorkflowNode[],
  edges: WorkflowEdge[],
): Promise<WorkflowGraph> {
  return authedRequest(
    `/api/workspaces/${workspaceId}/workflows/${workflowId}`,
    token,
    { method: "PUT", body: JSON.stringify({ nodes, edges }) },
  );
}

export function runWorkflow(
  token: string,
  workspaceId: string,
  workflowId: string,
): Promise<ExecutionResult> {
  return authedRequest(
    `/api/workspaces/${workspaceId}/workflows/${workflowId}/executions`,
    token,
    { method: "POST" },
  );
}

export function listMcpConnections(
  token: string,
  workspaceId: string,
): Promise<McpConnection[]> {
  return authedRequest(`/api/workspaces/${workspaceId}/mcp-connections`, token);
}

export function createMcpConnection(
  token: string,
  workspaceId: string,
  connection: { name: string; url: string; bearer_token?: string },
): Promise<McpConnection> {
  return authedRequest(`/api/workspaces/${workspaceId}/mcp-connections`, token, {
    method: "POST",
    body: JSON.stringify(connection),
  });
}

export function deleteMcpConnection(
  token: string,
  workspaceId: string,
  connectionId: string,
): Promise<{ deleted: true }> {
  return authedRequest(
    `/api/workspaces/${workspaceId}/mcp-connections/${connectionId}`,
    token,
    { method: "DELETE" },
  );
}

export function testMcpConnection(
  token: string,
  workspaceId: string,
  connectionId: string,
): Promise<McpTool[]> {
  return authedRequest(
    `/api/workspaces/${workspaceId}/mcp-connections/${connectionId}/test`,
    token,
    { method: "POST" },
  );
}

export function listApprovals(
  token: string,
  workspaceId: string,
): Promise<ApprovalGate[]> {
  return authedRequest(`/api/workspaces/${workspaceId}/approvals`, token);
}

export function approveGate(
  token: string,
  workspaceId: string,
  gateId: string,
): Promise<{ status: string }> {
  return authedRequest(
    `/api/workspaces/${workspaceId}/approvals/${gateId}/approve`,
    token,
    { method: "POST" },
  );
}

export function rejectGate(
  token: string,
  workspaceId: string,
  gateId: string,
): Promise<{ status: string }> {
  return authedRequest(
    `/api/workspaces/${workspaceId}/approvals/${gateId}/reject`,
    token,
    { method: "POST" },
  );
}

/**
 * Opens the live trace WebSocket for a run. The token travels as a query param, not a
 * header, because browsers don't allow custom headers on a WS handshake.
 */
export function openExecutionTrace(
  token: string,
  workspaceId: string,
  workflowId: string,
  executionId: string,
): WebSocket {
  const wsUrl = API_URL.replace(/^http/, "ws");
  return new WebSocket(
    `${wsUrl}/api/workspaces/${workspaceId}/workflows/${workflowId}/executions/${executionId}/trace?token=${encodeURIComponent(token)}`,
  );
}
