import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { StatusBadge } from "./StatusBadge";

describe("StatusBadge", () => {
  it("shows the idle indicator without a count", () => {
    render(<StatusBadge capturing={false} itemCount={0} />);
    expect(screen.getByText("Context Drop")).toBeInTheDocument();
    expect(screen.getByText("○")).toBeInTheDocument();
    expect(screen.queryByTestId("badge-count")).not.toBeInTheDocument();
  });

  it("shows the capturing indicator with the item count", () => {
    render(<StatusBadge capturing={true} itemCount={5} />);
    expect(screen.getByText("●")).toBeInTheDocument();
    expect(screen.getByTestId("badge-count")).toHaveTextContent("5");
  });

  it("shows the app version when given", () => {
    render(<StatusBadge capturing={false} itemCount={0} version="0.1.6" />);
    expect(screen.getByTestId("app-version")).toHaveTextContent("v0.1.6");
  });

  it("omits the version until it is known", () => {
    render(<StatusBadge capturing={false} itemCount={0} />);
    expect(screen.queryByTestId("app-version")).not.toBeInTheDocument();
  });
});
