import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { CapturePanel } from "./CapturePanel";

describe("CapturePanel", () => {
  it("shows Start when idle and calls onStart", async () => {
    const onStart = vi.fn();
    const onStop = vi.fn();
    render(<CapturePanel capturing={false} busy={false} onStart={onStart} onStop={onStop} />);
    const btn = screen.getByRole("button", { name: "Start Capture" });
    await userEvent.click(btn);
    expect(onStart).toHaveBeenCalledOnce();
    expect(onStop).not.toHaveBeenCalled();
  });

  it("shows Stop when capturing and calls onStop", async () => {
    const onStart = vi.fn();
    const onStop = vi.fn();
    render(<CapturePanel capturing={true} busy={false} onStart={onStart} onStop={onStop} />);
    await userEvent.click(screen.getByRole("button", { name: "Stop Capture" }));
    expect(onStop).toHaveBeenCalledOnce();
  });

  it("always shows the privacy note", () => {
    render(<CapturePanel capturing={false} busy={false} onStart={vi.fn()} onStop={vi.fn()} />);
    expect(screen.getByText(/only while Capture is ON/i)).toBeInTheDocument();
  });

  it("tells the user they can drag & drop files", () => {
    render(<CapturePanel capturing={false} busy={false} onStart={vi.fn()} onStop={vi.fn()} />);
    expect(screen.getByText(/drag & drop files/i)).toBeInTheDocument();
  });

  it("disables the button while busy", () => {
    render(<CapturePanel capturing={false} busy={true} onStart={vi.fn()} onStop={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Start Capture" })).toBeDisabled();
  });
});
