import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AppSnapshot, ShortcutStatus } from "../lib/api";
import { App } from "./App";

// Mock the whole command surface so the UI runs without a Tauri backend.
const mocks = vi.hoisted(() => ({
  getSnapshot: vi.fn(),
  startCapture: vi.fn(),
  captureClipboardNow: vi.fn(),
  stopCapture: vi.fn(),
  clearPacket: vi.fn(),
  undoLast: vi.fn(),
  saveSettings: vi.fn(),
  installIntegration: vi.fn(),
  installShortAlias: vi.fn(),
  openDataFolder: vi.fn(),
  privacyInfo: vi.fn(),
  listDisplays: vi.fn().mockResolvedValue([]),
  updateSettings: vi.fn(),
  beginRecording: vi.fn().mockResolvedValue(undefined),
  endRecording: vi.fn().mockResolvedValue(undefined),
  checkForUpdates: vi.fn(),
  installUpdate: vi.fn(),
  dismissUpdateNotice: vi.fn(),
}));

vi.mock("../lib/api", () => ({ api: mocks }));

function shortcut(
  action: ShortcutStatus["action"],
  accelerator: string,
  state: ShortcutStatus["state"] = "active",
): ShortcutStatus {
  return { action, accelerator, state, duplicateOf: null };
}

function snapshot(overrides: Partial<AppSnapshot> = {}): AppSnapshot {
  return {
    capturing: false,
    shortcuts: [
      shortcut("capture", "CommandOrControl+Shift+9"),
      shortcut("window", "CommandOrControl+Shift+0"),
      shortcut("onTop", "CommandOrControl+Shift+8"),
    ],
    currentDraft: null,
    lastDispatch: null,
    recentDispatches: [],
    readyCount: 0,
    settings: {
      globalShortcut: "CommandOrControl+Shift+9",
      showWindowShortcut: "CommandOrControl+Shift+0",
      onTopShortcut: "CommandOrControl+Shift+8",
      windowPlacement: "cursor",
      windowDisplay: "",
      alwaysOnTop: false,
      showOnAllDesktops: false,
      packetTtlHours: 24,
      maxItemBytes: 25 * 1024 * 1024,
      maxPacketBytes: 200 * 1024 * 1024,
      autoCleanup: true,
      shortAliasInstalled: false,
      autoCheckUpdates: true,
    },
    integrations: [],
    marketplaceGithub: "EarthLinkNetwork/context-drop",
    notice: null,
    appVersion: "9.9.9",
    update: {
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
      announce: false,
      asked: 0,
    },
    ...overrides,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  // jsdom has no scrolling; App scrolls to the top when the menu asks.
  window.scrollTo = vi.fn();
  mocks.dismissUpdateNotice.mockResolvedValue(undefined);
  mocks.startCapture.mockResolvedValue(undefined);
  mocks.captureClipboardNow.mockResolvedValue(undefined);
  mocks.stopCapture.mockResolvedValue(undefined);
});

describe("App", () => {
  // Tier A: an update found by the backend shows the banner; Install installs.
  it("shows a found update and installs it on request", async () => {
    mocks.installUpdate.mockResolvedValue(undefined);
    mocks.getSnapshot.mockResolvedValue(
      snapshot({
        update: { ...snapshot().update, state: "available", version: "0.1.11" },
      }),
    );
    render(<App />);
    const banner = await screen.findByTestId("update-banner");
    expect(banner).toHaveTextContent("v0.1.11 is available");
    await userEvent.click(screen.getByRole("button", { name: "Install & Restart" }));
    await waitFor(() => expect(mocks.installUpdate).toHaveBeenCalledOnce());
  });

  // Tier A: Check for Updates is on the first thing Settings shows (no tab to
  // find first) and calls the backend.
  it("checks for updates from Settings", async () => {
    mocks.checkForUpdates.mockResolvedValue(snapshot().update);
    mocks.getSnapshot.mockResolvedValue(snapshot());
    render(<App />);
    await userEvent.click(await screen.findByRole("tab", { name: "Settings" }));
    expect(screen.getByText("Context Drop v9.9.9")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Check for Updates" }));
    await waitFor(() => expect(mocks.checkForUpdates).toHaveBeenCalledOnce());
  });

  // Tier A: Check for Updates in the menu-bar menu with nothing newer: the
  // window says this version is the latest (it used to show nothing); OK
  // tells the backend to stop showing it.
  it("shows the result of a check asked for from the menu, until dismissed", async () => {
    const announced = snapshot({
      update: { ...snapshot().update, state: "upToDate", checkedAt: 1, announce: true },
    });
    mocks.getSnapshot.mockResolvedValue(announced);
    mocks.dismissUpdateNotice.mockImplementation(async () => {
      mocks.getSnapshot.mockResolvedValue(snapshot({ update: { ...announced.update, announce: false } }));
    });
    render(<App />);
    const result = await screen.findByTestId("update-check-result");
    expect(result).toHaveTextContent("Context Drop v9.9.9 is the latest version");
    await userEvent.click(screen.getByRole("button", { name: "OK" }));
    await waitFor(() => expect(mocks.dismissUpdateNotice).toHaveBeenCalledOnce());
    await waitFor(() => expect(screen.queryByTestId("update-check-result")).toBeNull());
  });

  // Tier A: a failed check asked for from the menu shows why; Try Again checks.
  it("shows a failed check asked for from the menu and retries it", async () => {
    mocks.checkForUpdates.mockResolvedValue(snapshot().update);
    mocks.getSnapshot.mockResolvedValue(
      snapshot({
        update: {
          ...snapshot().update,
          error: "Could not check for updates: offline",
          errorDuring: "check",
          announce: true,
        },
      }),
    );
    render(<App />);
    expect(await screen.findByTestId("update-check-result")).toHaveTextContent("offline");
    await userEvent.click(screen.getByRole("button", { name: "Try Again" }));
    await waitFor(() => expect(mocks.checkForUpdates).toHaveBeenCalledOnce());
  });

  // Tier A: the result is at the top of the page, so each time the menu asks
  // the page scrolls there (it used to stay wherever it was scrolled to, with
  // the result out of sight). Nothing scrolls when the menu was not used.
  it("scrolls to the top each time the menu asks for a check", async () => {
    // One window for the whole test: the 1.5 s poll delivers each snapshot.
    const asking = (asked: number) =>
      snapshot({ update: { ...snapshot().update, state: "upToDate", announce: true, asked } });
    const polled = async (n: number) =>
      waitFor(() => expect(mocks.getSnapshot.mock.calls.length).toBeGreaterThanOrEqual(n), {
        timeout: 4000,
      });
    mocks.getSnapshot.mockResolvedValue(snapshot());
    render(<App />);
    await screen.findByRole("tab", { name: "Settings" });
    expect(window.scrollTo).not.toHaveBeenCalled();

    mocks.getSnapshot.mockResolvedValue(asking(1));
    await screen.findByTestId("update-check-result", undefined, { timeout: 4000 });
    expect(window.scrollTo).toHaveBeenCalledTimes(1);
    expect(window.scrollTo).toHaveBeenLastCalledWith(0, 0);

    // Another poll with the same request does not scroll again…
    await polled(mocks.getSnapshot.mock.calls.length + 1);
    expect(window.scrollTo).toHaveBeenCalledTimes(1);

    // …a new request from the menu does.
    mocks.getSnapshot.mockResolvedValue(asking(2));
    await waitFor(() => expect(window.scrollTo).toHaveBeenCalledTimes(2), { timeout: 4000 });
  }, 15000);

  // Tier A: a check made in Settings is shown in Settings only — it ends an
  // earlier announcement instead of appearing on the Capture tab afterwards.
  it("ends the menu's announcement when Settings checks", async () => {
    const order: string[] = [];
    mocks.dismissUpdateNotice.mockImplementation(async () => void order.push("dismiss"));
    mocks.checkForUpdates.mockImplementation(async () => {
      order.push("check");
      return snapshot().update;
    });
    mocks.getSnapshot.mockResolvedValue(
      snapshot({ update: { ...snapshot().update, state: "upToDate", announce: true, asked: 1 } }),
    );
    render(<App />);
    await userEvent.click(await screen.findByRole("tab", { name: "Settings" }));
    await userEvent.click(screen.getByRole("button", { name: "Check for Updates" }));
    await waitFor(() => expect(order).toEqual(["dismiss", "check"]));
  });

  // Tier B: in Settings the Updates panel already shows the result, so the
  // window does not show it twice.
  it("shows the announced result once while Settings is open", async () => {
    mocks.getSnapshot.mockResolvedValue(
      snapshot({
        update: { ...snapshot().update, state: "upToDate", checkedAt: 1, announce: true },
      }),
    );
    render(<App />);
    await screen.findByTestId("update-check-result");
    await userEvent.click(screen.getByRole("tab", { name: "Settings" }));
    expect(screen.queryByTestId("update-check-result")).toBeNull();
    expect(screen.getByTestId("updates-status")).toHaveTextContent("You have the latest version");
  });

  it("renders the capturing state and item count", async () => {
    mocks.getSnapshot.mockResolvedValue(
      snapshot({
        capturing: true,
        currentDraft: {
          packetId: "p1",
          itemCount: 2,
          recentItems: [
            { id: "i1", kind: "image", mimeType: "image/png", byteSize: 4096 },
            { id: "i2", kind: "text", mimeType: "text/plain", byteSize: 20 },
          ],
        },
      }),
    );
    render(<App />);
    expect(await screen.findByText("●")).toBeInTheDocument();
    expect(screen.getByTestId("badge-count")).toHaveTextContent("2");
    expect(screen.getByTestId("item-count")).toHaveTextContent("2 items");
    // Capturing => the primary control is Stop.
    expect(screen.getByRole("button", { name: "Stop Capture" })).toBeInTheDocument();
  });

  it("starts capture when Start is clicked", async () => {
    mocks.getSnapshot.mockResolvedValue(snapshot({ capturing: false }));
    render(<App />);
    const start = await screen.findByRole("button", { name: "Start Capture" });
    await userEvent.click(start);
    await waitFor(() => expect(mocks.startCapture).toHaveBeenCalledOnce());
  });

  it("captures the clipboard once without starting capture", async () => {
    mocks.getSnapshot.mockResolvedValue(snapshot({ capturing: false }));
    render(<App />);
    const now = await screen.findByRole("button", { name: "Capture Clipboard Now" });
    await userEvent.click(now);
    await waitFor(() => expect(mocks.captureClipboardNow).toHaveBeenCalledOnce());
    expect(mocks.startCapture).not.toHaveBeenCalled();
  });

  it("shows the running app version in the header", async () => {
    mocks.getSnapshot.mockResolvedValue(snapshot());
    render(<App />);
    expect(await screen.findByTestId("app-version")).toHaveTextContent("v9.9.9");
  });

  it("shows an error state and recovers on Retry", async () => {
    mocks.getSnapshot.mockRejectedValueOnce("backend offline");
    render(<App />);
    // Error banner appears with the message.
    expect(await screen.findByText("backend offline")).toBeInTheDocument();
    const retry = screen.getByRole("button", { name: "Retry" });
    // Next call succeeds.
    mocks.getSnapshot.mockResolvedValue(snapshot({ capturing: false }));
    await userEvent.click(retry);
    expect(await screen.findByRole("button", { name: "Start Capture" })).toBeInTheDocument();
  });

  // Tier A: a shortcut that is set but not working shows a banner naming it,
  // visible without opening Settings.
  it("names a failed shortcut in the global banner", async () => {
    mocks.getSnapshot.mockResolvedValue(
      snapshot({
        shortcuts: [
          shortcut("capture", "CommandOrControl+Shift+9"),
          shortcut("window", "CommandOrControl+Shift+0", "failed"),
          shortcut("onTop", "CommandOrControl+Shift+8"),
        ],
      }),
    );
    render(<App />);
    expect(await screen.findByTestId("shortcut-conflict-global")).toHaveTextContent(
      "The “Show / hide window” shortcut is not active",
    );
  });

  // Tier A: when several fail the banner names each of them.
  it("names every broken shortcut", async () => {
    mocks.getSnapshot.mockResolvedValue(
      snapshot({
        shortcuts: [
          shortcut("capture", "CommandOrControl+Shift+9", "failed"),
          shortcut("window", "CommandOrControl+Shift+0"),
          shortcut("onTop", "CommandOrControl+Shift+9", "duplicate"),
        ],
      }),
    );
    render(<App />);
    expect(await screen.findByTestId("shortcut-conflict-global")).toHaveTextContent(
      "The “Start / Stop Capture” and “Always on Top” shortcuts are not active",
    );
  });

  // Tier A: a shortcut the user turned off is not a problem — no banner.
  it("shows no banner for shortcuts that are off", async () => {
    mocks.getSnapshot.mockResolvedValue(
      snapshot({
        shortcuts: [
          shortcut("capture", "", "off"),
          shortcut("window", "", "off"),
          shortcut("onTop", "", "off"),
        ],
      }),
    );
    render(<App />);
    expect(await screen.findByRole("button", { name: "Start Capture" })).toBeInTheDocument();
    expect(screen.queryByTestId("shortcut-conflict-global")).not.toBeInTheDocument();
  });

  it("splits Capture and Settings into tabs", async () => {
    mocks.getSnapshot.mockResolvedValue(snapshot({ capturing: false }));
    render(<App />);
    // Capture tab is the default: capture control + Last Dispatch are visible,
    // the settings form is not.
    expect(await screen.findByRole("button", { name: "Start Capture" })).toBeInTheDocument();
    expect(screen.getByText("Recent Dispatches")).toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: "Shortcuts" })).not.toBeInTheDocument();
    // Switch to Settings: its own tabs appear (Shortcuts first), capture hides.
    await userEvent.click(screen.getByRole("tab", { name: "Settings" }));
    expect(screen.getByRole("tab", { name: "Shortcuts" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByText("Start / Stop Capture")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Start Capture" })).not.toBeInTheDocument();
    // There is no Save button any more: everything saves automatically.
    expect(screen.queryByRole("button", { name: /Save/ })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("tab", { name: "Claude Code" }));
    expect(screen.getByText("Claude Code Setup")).toBeInTheDocument();
  });

  // Tier A: "Open Setup" lands on the Claude Code tab, not on Shortcuts.
  it("opens the Claude Code tab from the setup banner", async () => {
    mocks.getSnapshot.mockResolvedValue(snapshot({ integrations: [] }));
    render(<App />);
    await userEvent.click(await screen.findByRole("button", { name: "Open Setup" }));
    expect(screen.getByRole("tab", { name: "Claude Code" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByText("Claude Code Setup")).toBeInTheDocument();
  });

  it("shows a backend notice (e.g. a size-limit rejection reason)", async () => {
    mocks.getSnapshot.mockResolvedValue(
      snapshot({ notice: "Rejected item: 30000000 bytes exceeds the 26214400-byte item limit" }),
    );
    render(<App />);
    expect(await screen.findByTestId("notice-banner")).toHaveTextContent(/exceeds the .* item limit/);
  });
});
