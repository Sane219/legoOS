const API_URL = process.env.NEXT_PUBLIC_API_URL ?? "http://localhost:8080";

export interface AuthResponse {
  token: string;
}

export interface UserResponse {
  id: string;
  email: string;
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

async function request<T>(
  path: string,
  options: RequestInit = {},
): Promise<T> {
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
  return request("/api/auth/me", {
    headers: { Authorization: `Bearer ${token}` },
  });
}
