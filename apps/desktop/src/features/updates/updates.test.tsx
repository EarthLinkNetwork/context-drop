import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { UpdateStatus } from "../../lib/api";
import { UpdateBanner } from "./UpdateBanner";
import { UpdatesPanel } from "./UpdatesPanel";

function status(over: Partial<UpdateStatus> = {}): UpdateStatus {
  return {
    state: "idle",
    checking: false,
    version: null,
    notes: null,
    downloaded: 0,
    total: null,
    error: null,
    errorDuring: null,
    checkedAt: null,
    blockedReason: null,
    supported: true,
    ...over,
  };
}

describe("UpdateBanner", () => {
  // Tier A: nothing is shown until an update is found.
  it("is hidden while no update is known", () => {
    for (const state of ["idle", "checking", "upToDate"] as const) {
      const { container, unmount } = render(
        <UpdateBanner update={status({ state })} capturing={false} onInstall={vi.fn()} />,
      );
      expect(container).toBeEmptyDOMElement();
      unmount();
    }
  });

  // Tier A: an update found offers Install & Restart, which installs it.
  it("offers Install & Restart for a found update", async () => {
    const onInstall = vi.fn();
    render(
      <UpdateBanner
        update={status({ state: "available", version: "0.1.11" })}
        capturing={false}
        onInstall={onInstall}
      />,
    );
    expect(screen.getByTestId("update-banner")).toHaveTextContent("v0.1.11 is available");
    await userEvent.click(screen.getByRole("button", { name: "Install & Restart" }));
    expect(onInstall).toHaveBeenCalledOnce();
  });

  // Tier A: a copy that cannot be replaced (disk image, dev build) shows why
  // instead of an Install button that would fail.
  it("shows the reason instead of Install when the app cannot be replaced", () => {
    render(
      <UpdateBanner
        update={status({ state: "available", version: "0.1.11", blockedReason: "Move it to Applications." })}
        capturing={false}
        onInstall={vi.fn()}
      />,
    );
    expect(screen.queryByRole("button", { name: "Install & Restart" })).toBeNull();
    expect(screen.getByText("Move it to Applications.")).toBeInTheDocument();
  });

  // Tier A: a re-check while an update is already known keeps the banner
  // (the update does not vanish for the length of the check).
  it("keeps a found update on screen while re-checking", () => {
    render(
      <UpdateBanner
        update={status({ state: "available", checking: true, version: "0.1.11" })}
        capturing={false}
        onInstall={vi.fn()}
      />,
    );
    expect(screen.getByTestId("update-banner")).toHaveTextContent("v0.1.11 is available");
    expect(screen.getByRole("button", { name: "Install & Restart" })).toBeDisabled();
  });

  // Tier A: while capturing, the user is told the restart turns Capture off.
  it("warns that Capture turns off when capturing", () => {
    render(
      <UpdateBanner
        update={status({ state: "available", version: "0.1.11" })}
        capturing
        onInstall={vi.fn()}
      />,
    );
    expect(screen.getByText(/Capture turns off/)).toBeInTheDocument();
  });

  // Tier A: progress, and a failed install with its reason (Install stays).
  it("shows download progress and an install failure", () => {
    const { rerender } = render(
      <UpdateBanner
        update={status({ state: "downloading", version: "0.1.11", downloaded: 50, total: 200 })}
        capturing={false}
        onInstall={vi.fn()}
      />,
    );
    expect(screen.getByTestId("update-banner")).toHaveTextContent("Downloading v0.1.11… 25%");
    rerender(
      <UpdateBanner
        update={status({
          state: "available",
          version: "0.1.11",
          error: "Could not install the update: disk full",
          errorDuring: "install",
        })}
        capturing={false}
        onInstall={vi.fn()}
      />,
    );
    expect(screen.getByTestId("update-install-error")).toHaveTextContent("disk full");
    expect(screen.getByRole("button", { name: "Install & Restart" })).toBeInTheDocument();
  });
});

describe("UpdatesPanel", () => {
  // Tier A: Check for Updates checks; it is disabled while one runs.
  it("checks on request and not twice at once", async () => {
    const onCheck = vi.fn();
    const { rerender } = render(
      <UpdatesPanel appVersion="0.1.10" update={status()} onCheck={onCheck} onInstall={vi.fn()} />,
    );
    expect(screen.getByText("Context Drop v0.1.10")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Check for Updates" }));
    expect(onCheck).toHaveBeenCalledOnce();
    rerender(
      <UpdatesPanel
        appVersion="0.1.10"
        update={status({ state: "checking", checking: true })}
        onCheck={onCheck}
        onInstall={vi.fn()}
      />,
    );
    expect(screen.getByRole("button", { name: "Checking…" })).toBeDisabled();
  });

  // Tier A: each result is shown: latest, found (with Install), failure.
  it("shows the result of a check", async () => {
    const onInstall = vi.fn();
    const { rerender } = render(
      <UpdatesPanel
        appVersion="0.1.10"
        update={status({ state: "upToDate", checkedAt: Date.UTC(2026, 9, 15, 9, 0) })}
        onCheck={vi.fn()}
        onInstall={onInstall}
      />,
    );
    expect(screen.getByTestId("updates-status")).toHaveTextContent("You have the latest version");
    rerender(
      <UpdatesPanel
        appVersion="0.1.10"
        update={status({ state: "available", version: "0.1.11" })}
        onCheck={vi.fn()}
        onInstall={onInstall}
      />,
    );
    expect(screen.getByTestId("updates-status")).toHaveTextContent("Version 0.1.11 is available.");
    await userEvent.click(screen.getByRole("button", { name: "Install v0.1.11 & Restart" }));
    expect(onInstall).toHaveBeenCalledOnce();
    rerender(
      <UpdatesPanel
        appVersion="0.1.10"
        update={status({ error: "Could not check for updates: offline", errorDuring: "check" })}
        onCheck={vi.fn()}
        onInstall={onInstall}
      />,
    );
    expect(screen.getByTestId("updates-error")).toHaveTextContent("offline");
  });

  // Tier A: no update button on platforms without updates.
  it("explains when updates are not available on this platform", () => {
    render(
      <UpdatesPanel
        appVersion="0.1.10"
        update={status({ supported: false })}
        onCheck={vi.fn()}
        onInstall={vi.fn()}
      />,
    );
    expect(screen.queryByRole("button", { name: "Check for Updates" })).toBeNull();
    expect(screen.getByText(/macOS only/)).toBeInTheDocument();
  });
});
