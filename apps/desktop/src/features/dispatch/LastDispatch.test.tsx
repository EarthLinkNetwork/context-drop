import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { LastDispatch } from "./LastDispatch";

const base = {
  packetId: "p1",
  projectName: "prompt-flow",
  itemCount: 7,
  claimedAt: new Date().toISOString(),
  state: "CLAIMED",
};

describe("LastDispatch", () => {
  it("shows an empty state with no dispatch", () => {
    render(<LastDispatch dispatch={null} busy={false} onUndo={vi.fn()} />);
    expect(screen.getByText(/No dispatches yet/i)).toBeInTheDocument();
  });

  it("shows the project, item count, and an Undo button while CLAIMED", async () => {
    const onUndo = vi.fn();
    render(<LastDispatch dispatch={base} busy={false} onUndo={onUndo} />);
    expect(screen.getByTestId("dispatch-project")).toHaveTextContent("prompt-flow");
    expect(screen.getByText(/7 items/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Undo" }));
    expect(onUndo).toHaveBeenCalledOnce();
  });

  it("hides Undo once the packet is CONSUMED", () => {
    render(<LastDispatch dispatch={{ ...base, state: "CONSUMED" }} busy={false} onUndo={vi.fn()} />);
    expect(screen.queryByRole("button", { name: "Undo" })).not.toBeInTheDocument();
  });
});
