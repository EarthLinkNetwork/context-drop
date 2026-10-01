import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AppSnapshot } from "../lib/api";
import { App } from "./App";

// Mock the whole command surface so the UI runs without a Tauri backend.
const mocks = vi.hoisted(() => ({
  getSnapshot: vi.fn(),
  startCapture: vi.fn(),
  stopCapture: vi.fn(),
  clearPacket: vi.fn(),
  undoLast: vi.fn(),
  saveSettings: vi.fn(),
  installIntegration: vi.fn(),
  installShortAlias: vi.fn(),
  openDataFolder: vi.fn(),
  privacyInfo: vi.fn(),
}));

vi.mock("../lib/api", () => ({ api: mocks }));

function snapshot(overrides: Partial<AppSnapshot> = {}): AppSnapshot {
  return {
    capturing: false,
    shortcut: "CommandOrControl+Shift+9",
    shortcutRegistered: true,
    currentDraft: null,
    lastDispatch: null,
    readyCount: 0,
    settings: {
      globalShortcut: "CommandOrControl+Shift+9",
      packetTtlHours: 24,
      maxItemBytes: 25 * 1024 * 1024,
      maxPacketBytes: 200 * 1024 * 1024,
      autoCleanup: true,
      shortAliasInstalled: false,
    },
    integrations: [],
    marketplaceGithub: "EarthLinkNetwork/context-drop",
    notice: null,
    ...overrides,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.startCapture.mockResolvedValue(undefined);
  mocks.stopCapture.mockResolvedValue(undefined);
});

describe("App", () => {
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

  it("surfaces the shortcut-conflict warning from the snapshot", async () => {
    mocks.getSnapshot.mockResolvedValue(snapshot({ shortcutRegistered: false }));
    render(<App />);
    // The shortcut setting (and its conflict warning) live on the Settings tab.
    await userEvent.click(await screen.findByRole("tab", { name: "Settings" }));
    expect(await screen.findByTestId("shortcut-conflict")).toBeInTheDocument();
  });

  it("splits Capture and Settings into tabs", async () => {
    mocks.getSnapshot.mockResolvedValue(snapshot({ capturing: false }));
    render(<App />);
    // Capture tab is the default: capture control + Last Dispatch are visible,
    // the settings form is not.
    expect(await screen.findByRole("button", { name: "Start Capture" })).toBeInTheDocument();
    expect(screen.getByText("Last Dispatch")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Save Settings" })).not.toBeInTheDocument();
    // Switch to Settings: the setup + settings form appear, capture control hides.
    await userEvent.click(screen.getByRole("tab", { name: "Settings" }));
    expect(screen.getByRole("button", { name: "Save Settings" })).toBeInTheDocument();
    expect(screen.getByText("Claude Code Setup")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Start Capture" })).not.toBeInTheDocument();
  });

  it("shows a backend notice (e.g. a size-limit rejection reason)", async () => {
    mocks.getSnapshot.mockResolvedValue(
      snapshot({ notice: "Rejected item: 30000000 bytes exceeds the 26214400-byte item limit" }),
    );
    render(<App />);
    expect(await screen.findByTestId("notice-banner")).toHaveTextContent(/exceeds the .* item limit/);
  });
});
