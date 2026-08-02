import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import McpConnectionsPage from "./page";
import { ApiError } from "@/lib/api";

const push = vi.fn();
const listMcpConnections = vi.fn();
const createMcpConnection = vi.fn();
const deleteMcpConnection = vi.fn();
const testMcpConnection = vi.fn();

const router = { push };
const routeParams = { workspaceId: "workspace-1" };

vi.mock("next/navigation", () => ({
  useRouter: () => router,
  useParams: () => routeParams,
}));

vi.mock("@/lib/api", async () => {
  const actual = await vi.importActual<typeof import("@/lib/api")>("@/lib/api");
  return {
    ...actual,
    listMcpConnections: (...args: unknown[]) => listMcpConnections(...args),
    createMcpConnection: (...args: unknown[]) => createMcpConnection(...args),
    deleteMcpConnection: (...args: unknown[]) => deleteMcpConnection(...args),
    testMcpConnection: (...args: unknown[]) => testMcpConnection(...args),
  };
});

const connection = {
  id: "conn-1",
  name: "My MCP",
  url: "https://mcp.example.com",
  has_token: true,
  created_at: "2026-01-01T00:00:00Z",
};

afterEach(() => {
  window.localStorage.clear();
  vi.clearAllMocks();
});

describe("McpConnectionsPage", () => {
  it("renders the connections list", async () => {
    window.localStorage.setItem("legoos_token", "test-token");
    listMcpConnections.mockResolvedValueOnce([connection]);

    render(<McpConnectionsPage />);

    expect(await screen.findByText("My MCP")).toBeInTheDocument();
    expect(screen.getByText("https://mcp.example.com")).toBeInTheDocument();
    expect(screen.getByText("Token saved")).toBeInTheDocument();
  });

  it("submits the add-connection form and calls the create API", async () => {
    window.localStorage.setItem("legoos_token", "test-token");
    listMcpConnections.mockResolvedValueOnce([]);
    createMcpConnection.mockResolvedValueOnce({
      id: "conn-2",
      name: "New Conn",
      url: "https://new.example.com",
      has_token: false,
      created_at: "2026-01-02T00:00:00Z",
    });

    render(<McpConnectionsPage />);

    await screen.findByText("No MCP connections yet.");

    fireEvent.change(screen.getByPlaceholderText("Name"), {
      target: { value: "New Conn" },
    });
    fireEvent.change(screen.getByPlaceholderText("https://example.com/mcp"), {
      target: { value: "https://new.example.com" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add connection" }));

    await waitFor(() =>
      expect(createMcpConnection).toHaveBeenCalledWith("test-token", "workspace-1", {
        name: "New Conn",
        url: "https://new.example.com",
      }),
    );
    expect(await screen.findByText("New Conn")).toBeInTheDocument();
  });

  it("calls the delete API when the delete button is clicked", async () => {
    window.localStorage.setItem("legoos_token", "test-token");
    listMcpConnections.mockResolvedValueOnce([connection]);
    deleteMcpConnection.mockResolvedValueOnce({ deleted: true });

    render(<McpConnectionsPage />);

    await screen.findByText("My MCP");
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));

    await waitFor(() =>
      expect(deleteMcpConnection).toHaveBeenCalledWith(
        "test-token",
        "workspace-1",
        "conn-1",
      ),
    );
    await waitFor(() => expect(screen.queryByText("My MCP")).not.toBeInTheDocument());
  });

  it("shows returned tools when the test-connection button is clicked", async () => {
    window.localStorage.setItem("legoos_token", "test-token");
    listMcpConnections.mockResolvedValueOnce([connection]);
    testMcpConnection.mockResolvedValueOnce([
      { name: "search", description: "Searches things" },
    ]);

    render(<McpConnectionsPage />);

    await screen.findByText("My MCP");
    fireEvent.click(screen.getByRole("button", { name: "Test" }));

    expect(await screen.findByText("search")).toBeInTheDocument();
    expect(screen.getByText(/Searches things/)).toBeInTheDocument();
  });

  it("shows an error message when the test API call fails", async () => {
    window.localStorage.setItem("legoos_token", "test-token");
    listMcpConnections.mockResolvedValueOnce([connection]);
    testMcpConnection.mockRejectedValueOnce(new ApiError("server unreachable", 400));

    render(<McpConnectionsPage />);

    await screen.findByText("My MCP");
    fireEvent.click(screen.getByRole("button", { name: "Test" }));

    expect(await screen.findByText("server unreachable")).toBeInTheDocument();
  });
});
