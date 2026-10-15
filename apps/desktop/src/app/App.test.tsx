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
    },
    ...overrides,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
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

  // Tier A: Settings → Advanced → Check for Updates calls the backend.
  it("checks for updates from Settings", async () => {
    mocks.checkForUpdates.mockResolvedValue(snapshot().update);
    mocks.getSnapshot.mockResolvedValue(snapshot());
    render(<App />);
    await userEvent.click(await screen.findByRole("tab", { name: "Settings" }));
    await userEvent.click(screen.getByRole("tab", { name: "Advanced" }));
    await userEvent.click(screen.getByRole("button", { name: "Check for Updates" }));
    await waitFor(() => expect(mocks.checkForUpdates).toHaveBeenCalledOnce());
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
