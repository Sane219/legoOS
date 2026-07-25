import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Nav } from "./Nav";

vi.mock("next/navigation", () => ({
  useRouter: () => ({ push: vi.fn(), replace: vi.fn() }),
}));

afterEach(() => {
  window.localStorage.clear();
});

describe("Nav", () => {
  it("shows log in / sign up links when logged out", () => {
    render(<Nav />);

    expect(screen.getByText("Log in")).toBeInTheDocument();
    expect(screen.getByText("Sign up")).toBeInTheDocument();
    expect(screen.queryByText("Log out")).not.toBeInTheDocument();
  });

  it("shows dashboard / log out when a token is present", () => {
    window.localStorage.setItem("legoos_token", "test-token");

    render(<Nav />);

    expect(screen.getByText("Dashboard")).toBeInTheDocument();
    expect(screen.getByText("Log out")).toBeInTheDocument();
    expect(screen.queryByText("Sign up")).not.toBeInTheDocument();
  });
});
