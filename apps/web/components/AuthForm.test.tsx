import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AuthForm } from "./AuthForm";
import { ApiError } from "@/lib/api";

const push = vi.fn();
const login = vi.fn();
const register = vi.fn();

vi.mock("next/navigation", () => ({
  useRouter: () => ({ push }),
}));

vi.mock("@/lib/api", async () => {
  const actual = await vi.importActual<typeof import("@/lib/api")>("@/lib/api");
  return {
    ...actual,
    login: (...args: unknown[]) => login(...args),
    register: (...args: unknown[]) => register(...args),
  };
});

afterEach(() => {
  window.localStorage.clear();
  vi.clearAllMocks();
});

describe("AuthForm", () => {
  it("logs in and redirects to the dashboard on success", async () => {
    login.mockResolvedValueOnce({ token: "test-token" });

    render(<AuthForm mode="login" />);

    fireEvent.change(screen.getByLabelText("Email"), {
      target: { value: "user@example.com" },
    });
    fireEvent.change(screen.getByLabelText("Password"), {
      target: { value: "hunter22222" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Log in" }));

    await waitFor(() => expect(push).toHaveBeenCalledWith("/dashboard"));
    expect(window.localStorage.getItem("legoos_token")).toBe("test-token");
  });

  it("shows an error message when the API call fails", async () => {
    login.mockRejectedValueOnce(new ApiError("invalid credentials", 401));

    render(<AuthForm mode="login" />);

    fireEvent.change(screen.getByLabelText("Email"), {
      target: { value: "user@example.com" },
    });
    fireEvent.change(screen.getByLabelText("Password"), {
      target: { value: "wrong-password" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Log in" }));

    expect(await screen.findByText("invalid credentials")).toBeInTheDocument();
    expect(push).not.toHaveBeenCalled();
  });
});
