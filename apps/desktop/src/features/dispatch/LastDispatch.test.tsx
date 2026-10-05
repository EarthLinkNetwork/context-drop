import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { LastDispatchInfo } from "../../lib/api";
import { LastDispatch } from "./LastDispatch";

const base: LastDispatchInfo = {
  packetId: "p1",
  projectName: "prompt-flow",
  itemCount: 7,
  claimedAt: new Date().toISOString(),
  state: "CLAIMED",
  sessionId: "11111111-2222-3333-4444-5555550abcde",
  cwd: "/Users/u/dev/eln/prompt-flow",
  configDir: "/Users/u/.claude-accounts/neko",
  note: "原因を調べて直して",
  terminal: "iTerm2 w0t2p0",
};

describe("LastDispatch", () => {
  it("shows an empty state with no dispatch", () => {
    render(<LastDispatch dispatches={[]} busy={false} onUndo={vi.fn()} />);
    expect(screen.getByText(/No dispatches yet/i)).toBeInTheDocument();
  });

  it("shows the project, item count, session details, and an Undo button while CLAIMED", async () => {
    const onUndo = vi.fn();
    render(<LastDispatch dispatches={[base]} busy={false} onUndo={onUndo} />);
    expect(screen.getByTestId("dispatch-project")).toHaveTextContent("prompt-flow");
    expect(screen.getByTestId("dispatch-note")).toHaveTextContent("原因を調べて直して");
    const meta = screen.getByText(/7 items/);
    expect(meta).toHaveTextContent("iTerm2 w0t2p0");
    expect(meta).toHaveTextContent("session …0abcde");
    expect(meta).toHaveTextContent("neko");
    expect(meta).toHaveTextContent("…/eln/prompt-flow");
    await userEvent.click(screen.getByRole("button", { name: "Undo" }));
    expect(onUndo).toHaveBeenCalledOnce();
  });

  it("hides Undo once the packet is CONSUMED", () => {
    render(
      <LastDispatch dispatches={[{ ...base, state: "CONSUMED" }]} busy={false} onUndo={vi.fn()} />,
    );
    expect(screen.queryByRole("button", { name: "Undo" })).not.toBeInTheDocument();
  });

  it("shows an undone claim as RELEASED without Undo", () => {
    render(
      <LastDispatch dispatches={[{ ...base, state: "RELEASED" }]} busy={false} onUndo={vi.fn()} />,
    );
    expect(screen.getByText("RELEASED")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Undo" })).not.toBeInTheDocument();
  });

  it("lists every dispatch newest first, offering Undo only on the newest", () => {
    const list = Array.from({ length: 10 }, (_, i) => ({
      ...base,
      packetId: `p${i}`,
      state: "PROCESSING",
      terminal: `iTerm2 w0t${i}p0`,
      note: i === 3 ? null : `task ${i}`,
    }));
    render(<LastDispatch dispatches={list} busy={false} onUndo={vi.fn()} />);
    const entries = screen.getAllByTestId("dispatch-entry");
    expect(entries).toHaveLength(10);
    expect(entries[0]).toHaveTextContent("task 0");
    expect(entries[0]).toHaveTextContent("iTerm2 w0t0p0");
    expect(entries[9]).toHaveTextContent("iTerm2 w0t9p0");
    expect(within(entries[3]).queryByTestId("dispatch-note")).not.toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Undo" })).toHaveLength(1);
    expect(within(entries[0]).getByRole("button", { name: "Undo" })).toBeInTheDocument();
  });

  it("omits unknown terminal and account", () => {
    render(
      <LastDispatch
        dispatches={[{ ...base, terminal: null, configDir: null, note: null }]}
        busy={false}
        onUndo={vi.fn()}
      />,
    );
    const meta = screen.getByText(/7 items/);
    expect(meta).not.toHaveTextContent("iTerm2");
    expect(meta).not.toHaveTextContent("neko");
  });
});
