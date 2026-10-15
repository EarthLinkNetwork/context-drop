import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { DisplayInfo, Settings, ShortcutStatus } from "../../lib/api";
import { SettingsView, type SettingsTab } from "./SettingsView";

const settings: Settings = {
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
};

const DISPLAYS: DisplayInfo[] = [
  { key: "Built-in@0,0", name: "Built-in", width: 1512, height: 982, primary: true },
  { key: "DELL U2720Q@1512,-200", name: "DELL U2720Q", width: 2560, height: 1440, primary: false },
];

function active(action: ShortcutStatus["action"], accelerator: string): ShortcutStatus {
  return { action, accelerator, state: "active", duplicateOf: null };
}

function setup(over: { settings?: Partial<Settings>; tab?: SettingsTab; shortcuts?: ShortcutStatus[] } = {}) {
  const onPatch = vi.fn().mockResolvedValue(null);
  const onBeginRecording = vi.fn();
  const onEndRecording = vi.fn();
  const pressed: { fire?: (acc: string) => void; end?: () => void } = {};
  const subscribeRecording = vi.fn((onPressed: (a: string) => void, onEnded: () => void) => {
    pressed.fire = onPressed;
    pressed.end = onEnded;
    return () => {
      pressed.fire = undefined;
      pressed.end = undefined;
    };
  });
  const props = {
    tab: over.tab ?? "shortcuts",
    onTabChange: vi.fn(),
    settings: { ...settings, ...over.settings },
    shortcuts: over.shortcuts ?? [
      active("capture", settings.globalShortcut),
      active("window", settings.showWindowShortcut),
      active("onTop", settings.onTopShortcut),
    ],
    platform: "mac" as const,
    onPatch,
    onListDisplays: vi.fn().mockResolvedValue(DISPLAYS),
    onBeginRecording,
    onEndRecording,
    subscribeRecording,
    onOpenDataFolder: vi.fn(),
    claudeSetup: <div>Claude Code Setup</div>,
  };
  const view = render(<SettingsView {...props} />);
  return { ...props, pressed, view };
}

function row(label: string) {
  return screen.getByTestId(`shortcut-${label}`);
}

function within(el: HTMLElement) {
  return {
    button: (name: string) =>
      Array.from(el.querySelectorAll("button")).find((b) => b.textContent === name)!,
    keys: () => el.querySelector(".shortcut-keys")!.textContent,
  };
}

describe("Shortcuts tab", () => {
  it("shows each shortcut as symbols", () => {
    setup();
    expect(within(row("Start / Stop Capture")).keys()).toBe("⌘⇧9");
    expect(within(row("Always on Top")).keys()).toBe("⌘⇧8");
  });

  // Tier A: Change → press keys saves exactly that combination, at once
  // (no Save button), and tells the backend the recording ended.
  it("records a pressed combination and saves it", async () => {
    const p = setup();
    const r = within(row("Always on Top"));
    await userEvent.click(r.button("Change"));
    expect(p.onBeginRecording).toHaveBeenCalledOnce();
    expect(r.keys()).toBe("Press keys…");
    fireEvent.keyDown(window, { code: "KeyT", metaKey: true, altKey: true });
    await waitFor(() =>
      expect(p.onPatch).toHaveBeenCalledWith({ onTopShortcut: "CommandOrControl+Alt+T" }),
    );
    expect(p.onEndRecording).toHaveBeenCalled();
  });

  // Tier A: Enter (no modifier) is never saved as a shortcut; recording goes
  // on with a reason, and Esc then cancels without saving.
  it("does not take Enter, and Esc cancels", async () => {
    const p = setup();
    const r = within(row("Show / hide window"));
    await userEvent.click(r.button("Change"));
    fireEvent.keyDown(window, { code: "Enter" });
    expect(screen.getByRole("alert")).toHaveTextContent("Include ⌘, ⌃ or ⌥");
    fireEvent.keyDown(window, { code: "Escape" });
    expect(p.onPatch).not.toHaveBeenCalled();
    expect(p.onEndRecording).toHaveBeenCalled();
    expect(r.keys()).toBe("⌘⇧0");
  });

  // Tier A: a combination another of the app's shortcuts uses is refused
  // (named), and recording continues.
  it("refuses a combination already used by another shortcut", async () => {
    const p = setup();
    const r = within(row("Always on Top"));
    await userEvent.click(r.button("Change"));
    fireEvent.keyDown(window, { code: "Digit9", metaKey: true, shiftKey: true });
    expect(screen.getByRole("alert")).toHaveTextContent("Already used for “Start / Stop Capture”");
    expect(p.onPatch).not.toHaveBeenCalled();
    expect(r.keys()).toBe("⌘⇧9");
  });

  // Tier A: pressing a registered shortcut while recording reaches the page
  // only through the backend event — it is recorded, not acted on.
  it("records a registered shortcut forwarded by the backend", async () => {
    const p = setup({ settings: { onTopShortcut: "" } });
    await userEvent.click(within(row("Show / hide window")).button("Change"));
    expect(p.subscribeRecording).toHaveBeenCalled();
    act(() => p.pressed.fire?.("CommandOrControl+Shift+0"));
    await waitFor(() =>
      expect(p.onPatch).toHaveBeenCalledWith({ showWindowShortcut: "CommandOrControl+Shift+0" }),
    );
  });

  // Tier A: when the backend ends the recording (window lost focus), the row
  // stops showing "recording" so the UI never lies about it.
  it("stops recording when the backend says it ended", async () => {
    const p = setup();
    const r = within(row("Start / Stop Capture"));
    await userEvent.click(r.button("Change"));
    act(() => p.pressed.end?.());
    expect(r.keys()).toBe("⌘⇧9");
    expect(r.button("Change")).toBeDefined();
  });

  // Tier A: Clear turns a shortcut off ("" saved) and shows "Off"; Reset puts
  // the default back.
  it("clears to off and resets to the default", async () => {
    const p = setup({ settings: { globalShortcut: "CommandOrControl+Alt+K" } });
    const r = within(row("Start / Stop Capture"));
    await userEvent.click(r.button("Clear"));
    expect(p.onPatch).toHaveBeenLastCalledWith({ globalShortcut: "" });
    await userEvent.click(r.button("Reset"));
    expect(p.onPatch).toHaveBeenLastCalledWith({ globalShortcut: "CommandOrControl+Shift+9" });
  });

  it("shows Off for an empty shortcut and disables Clear", () => {
    setup({ settings: { onTopShortcut: "" } });
    const r = within(row("Always on Top"));
    expect(r.keys()).toBe("Off");
    expect(r.button("Clear")).toBeDisabled();
  });

  // Tier A: Reset is refused when the default is taken by another shortcut.
  it("refuses Reset when the default is in use elsewhere", async () => {
    const p = setup({
      settings: { globalShortcut: "CommandOrControl+Shift+8", onTopShortcut: "" },
    });
    await userEvent.click(within(row("Always on Top")).button("Reset"));
    expect(p.onPatch).not.toHaveBeenCalled();
    expect(screen.getByRole("alert")).toHaveTextContent("already used for “Start / Stop Capture”");
  });

  // Tier A: a duplicate / failed status is explained on the row.
  it("explains why a shortcut is not active", () => {
    setup({
      shortcuts: [
        active("capture", settings.globalShortcut),
        { action: "window", accelerator: "x", state: "failed", duplicateOf: null },
        { action: "onTop", accelerator: "y", state: "duplicate", duplicateOf: "Start / Stop Capture" },
      ],
    });
    expect(row("Show / hide window")).toHaveTextContent("another app may be using");
    expect(row("Always on Top")).toHaveTextContent("same as “Start / Stop Capture”");
  });
});

describe("Window tab", () => {
  // Tier A: each change saves just its own field, immediately.
  it("auto-saves toggles and the placement", async () => {
    const p = setup({ tab: "window" });
    await userEvent.click(screen.getByLabelText("Always on top"));
    expect(p.onPatch).toHaveBeenLastCalledWith({ alwaysOnTop: true });
    await userEvent.click(screen.getByLabelText("Show on all desktops"));
    expect(p.onPatch).toHaveBeenLastCalledWith({ showOnAllDesktops: true });
    await userEvent.selectOptions(screen.getByLabelText("Open window on"), "last");
    expect(p.onPatch).toHaveBeenLastCalledWith({ windowPlacement: "last" });
    expect(await screen.findAllByText("Saved")).not.toHaveLength(0);
  });

  it("lists displays and saves the chosen one", async () => {
    const p = setup({ tab: "window", settings: { windowPlacement: "display" } });
    await screen.findByRole("option", { name: /DELL U2720Q/ });
    await userEvent.selectOptions(screen.getByLabelText("Display"), "DELL U2720Q@1512,-200");
    expect(p.onPatch).toHaveBeenLastCalledWith({ windowDisplay: "DELL U2720Q@1512,-200" });
  });

  it("marks a saved display that is not connected", async () => {
    setup({ tab: "window", settings: { windowPlacement: "display", windowDisplay: "LG@-1920,0" } });
    expect(await screen.findByRole("option", { name: "LG (not connected)" })).toBeInTheDocument();
    expect(await screen.findByTestId("display-missing")).toBeInTheDocument();
  });
});

describe("Advanced tab", () => {
  // Tier A: a number is saved when the field is left, converted to bytes.
  it("saves a number on blur", async () => {
    const p = setup({ tab: "advanced" });
    const f = screen.getByLabelText("Max item size (MB)");
    fireEvent.change(f, { target: { value: "30" } });
    expect(p.onPatch).not.toHaveBeenCalled();
    fireEvent.blur(f);
    await waitFor(() => expect(p.onPatch).toHaveBeenCalledWith({ maxItemBytes: 30 * 1024 * 1024 }));
  });

  // Tier A: an invalid number is not saved; the field goes back.
  it("reverts an invalid number instead of saving it", () => {
    const p = setup({ tab: "advanced" });
    const f = screen.getByLabelText("Packet TTL (hours)");
    fireEvent.change(f, { target: { value: "0" } });
    fireEvent.blur(f);
    expect(p.onPatch).not.toHaveBeenCalled();
    expect(f).toHaveValue(24);
  });

  it("saves the auto-cleanup toggle at once", async () => {
    const p = setup({ tab: "advanced" });
    await userEvent.click(screen.getByLabelText("Auto cleanup"));
    expect(p.onPatch).toHaveBeenLastCalledWith({ autoCleanup: false });
  });
});

// Tier A: a failed save is shown in the tab (not silently lost).
it("shows a save error", async () => {
  const p = setup({ tab: "window" });
  p.onPatch.mockResolvedValueOnce("database is locked");
  await userEvent.click(screen.getByLabelText("Always on top"));
  expect(await screen.findByTestId("autosave-error")).toHaveTextContent("database is locked");
});
