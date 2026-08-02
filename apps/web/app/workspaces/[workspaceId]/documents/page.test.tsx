import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import DocumentsPage from "./page";
import { ApiError } from "@/lib/api";

const push = vi.fn();
const listDocuments = vi.fn();
const createDocument = vi.fn();
const deleteDocument = vi.fn();

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
    listDocuments: (...args: unknown[]) => listDocuments(...args),
    createDocument: (...args: unknown[]) => createDocument(...args),
    deleteDocument: (...args: unknown[]) => deleteDocument(...args),
  };
});

const readyDoc = {
  id: "doc-1",
  name: "Handbook",
  status: "ready",
  error: null,
  created_at: "2026-01-01T00:00:00Z",
};

const failedDoc = {
  id: "doc-2",
  name: "Broken",
  status: "failed",
  error: "embedding failed",
  created_at: "2026-01-01T00:00:00Z",
};

afterEach(() => {
  window.localStorage.clear();
  vi.clearAllMocks();
});

describe("DocumentsPage", () => {
  it("renders the document list with status and error", async () => {
    window.localStorage.setItem("legoos_token", "test-token");
    listDocuments.mockResolvedValueOnce([readyDoc, failedDoc]);

    render(<DocumentsPage />);

    expect(await screen.findByText("Handbook")).toBeInTheDocument();
    expect(screen.getByText("ready")).toBeInTheDocument();
    expect(screen.getByText("Broken")).toBeInTheDocument();
    expect(screen.getByText("failed")).toBeInTheDocument();
    expect(screen.getByText("embedding failed")).toBeInTheDocument();
  });

  it("submits the upload form and calls the create API", async () => {
    window.localStorage.setItem("legoos_token", "test-token");
    listDocuments.mockResolvedValueOnce([]);
    createDocument.mockResolvedValueOnce({
      id: "doc-3",
      name: "New Doc",
      status: "pending",
      error: null,
      created_at: "2026-01-02T00:00:00Z",
    });

    render(<DocumentsPage />);

    await screen.findByText("No documents yet.");

    fireEvent.change(screen.getByPlaceholderText("Document name"), {
      target: { value: "New Doc" },
    });
    fireEvent.change(screen.getByPlaceholderText("Paste document content..."), {
      target: { value: "some content" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Upload document" }));

    await waitFor(() =>
      expect(createDocument).toHaveBeenCalledWith("test-token", "workspace-1", {
        name: "New Doc",
        content: "some content",
      }),
    );
    expect(await screen.findByText("New Doc")).toBeInTheDocument();
  });

  it("calls the delete API when the delete button is clicked", async () => {
    window.localStorage.setItem("legoos_token", "test-token");
    listDocuments.mockResolvedValueOnce([readyDoc]);
    deleteDocument.mockResolvedValueOnce({ deleted: true });

    render(<DocumentsPage />);

    await screen.findByText("Handbook");
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));

    await waitFor(() =>
      expect(deleteDocument).toHaveBeenCalledWith(
        "test-token",
        "workspace-1",
        "doc-1",
      ),
    );
    await waitFor(() => expect(screen.queryByText("Handbook")).not.toBeInTheDocument());
  });

  it("shows a create error via ApiError message", async () => {
    window.localStorage.setItem("legoos_token", "test-token");
    listDocuments.mockResolvedValueOnce([]);
    createDocument.mockRejectedValueOnce(new ApiError("name is required", 400));

    render(<DocumentsPage />);

    await screen.findByText("No documents yet.");
    fireEvent.change(screen.getByPlaceholderText("Document name"), {
      target: { value: "New Doc" },
    });
    fireEvent.change(screen.getByPlaceholderText("Paste document content..."), {
      target: { value: "some content" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Upload document" }));

    expect(await screen.findByText("name is required")).toBeInTheDocument();
  });

  describe("polling", () => {
    beforeEach(() => {
      vi.useFakeTimers();
    });

    afterEach(() => {
      vi.useRealTimers();
    });

    it("polls the list while a document is pending and stops once it's ready", async () => {
      window.localStorage.setItem("legoos_token", "test-token");
      const pendingDoc = { ...readyDoc, status: "pending" };
      listDocuments.mockResolvedValueOnce([pendingDoc]);

      render(<DocumentsPage />);

      // Flush the mount effect's initial listDocuments().then(...) microtasks
      // without using findByText/waitFor, which poll via real setTimeout and
      // don't play well with fake timers.
      await act(async () => {
        await Promise.resolve();
        await Promise.resolve();
      });
      expect(screen.getByText("pending")).toBeInTheDocument();
      expect(listDocuments).toHaveBeenCalledTimes(1);

      listDocuments.mockResolvedValueOnce([readyDoc]);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(2000);
      });
      expect(listDocuments).toHaveBeenCalledTimes(2);
      expect(screen.getByText("ready")).toBeInTheDocument();

      // No longer pending, so a further tick must not trigger another poll.
      await act(async () => {
        await vi.advanceTimersByTimeAsync(2000);
      });
      expect(listDocuments).toHaveBeenCalledTimes(2);
    });
  });
});
