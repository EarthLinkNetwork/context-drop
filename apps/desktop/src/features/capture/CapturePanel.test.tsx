import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { CapturePanel } from "./CapturePanel";

describe("CapturePanel", () => {
  it("shows Start when idle and calls onStart", async () => {
    const onStart = vi.fn();
    const onStop = vi.fn();
    render(<CapturePanel capturing={false} busy={false} onStart={onStart} onStop={onStop} onCaptureNow={vi.fn()} />);
    const btn = screen.getByRole("button", { name: "Start Capture" });
    await userEvent.click(btn);
    expect(onStart).toHaveBeenCalledOnce();
    expect(onStop).not.toHaveBeenCalled();
  });

  it("shows Stop when capturing and calls onStop", async () => {
    const onStart = vi.fn();
    const onStop = vi.fn();
    render(<CapturePanel capturing={true} busy={false} onStart={onStart} onStop={onStop} onCaptureNow={vi.fn()} />);
    await userEvent.click(screen.getByRole("button", { name: "Stop Capture" }));
    expect(onStop).toHaveBeenCalledOnce();
  });

  it.each([false, true])(
    "offers Capture Clipboard Now (capturing=%s) and calls onCaptureNow only",
    async (capturing) => {
      const onStart = vi.fn();
      const onStop = vi.fn();
      const onCaptureNow = vi.fn();
      render(
        <CapturePanel
          capturing={capturing}
          busy={false}
          onStart={onStart}
          onStop={onStop}
          onCaptureNow={onCaptureNow}
        />,
      );
      await userEvent.click(screen.getByRole("button", { name: "Capture Clipboard Now" }));
      expect(onCaptureNow).toHaveBeenCalledOnce();
      expect(onStart).not.toHaveBeenCalled();
      expect(onStop).not.toHaveBeenCalled();
    },
  );

  it("disables Capture Clipboard Now while busy", () => {
    render(
      <CapturePanel
        capturing={false}
        busy={true}
        onStart={vi.fn()}
        onStop={vi.fn()}
        onCaptureNow={vi.fn()}
      />,
    );
    expect(screen.getByRole("button", { name: "Capture Clipboard Now" })).toBeDisabled();
  });

  it("always shows the privacy note", () => {
    render(<CapturePanel capturing={false} busy={false} onStart={vi.fn()} onStop={vi.fn()} onCaptureNow={vi.fn()} />);
    expect(screen.getByText(/only while Capture is ON/i)).toBeInTheDocument();
  });

  it("tells the user they can drag & drop files", () => {
    render(<CapturePanel capturing={false} busy={false} onStart={vi.fn()} onStop={vi.fn()} onCaptureNow={vi.fn()} />);
    expect(screen.getByText(/drag & drop files/i)).toBeInTheDocument();
  });

  it("disables the button while busy", () => {
    render(<CapturePanel capturing={false} busy={true} onStart={vi.fn()} onStop={vi.fn()} onCaptureNow={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Start Capture" })).toBeDisabled();
  });
});
