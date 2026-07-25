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
