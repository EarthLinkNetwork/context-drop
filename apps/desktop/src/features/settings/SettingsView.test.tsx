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
  autoCheckUpdates: true,
};

const DISPLAYS: DisplayInfo[] = [
  { key: "Built-in@0,0", name: "Built-in", width: 1512, height: 982, primary: true },
  { key: "DELL U2720Q@1512,-200", name: "DELL U2720Q", width: 2560, height: 1440, primary: false },
];

function active(action: ShortcutStatus["action"], accelerator: string): ShortcutStatus {
  return { action, accelerator, state: "active", duplicateOf: null };
}

function setup(
  over: {
    settings?: Partial<Settings>;
    tab?: SettingsTab;
    shortcuts?: ShortcutStatus[];
    updatesSupported?: boolean;
  } = {},
) {
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
    visible: true,
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
    updates: <div>Updates panel</div>,
    updatesSupported: over.updatesSupported ?? true,
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

describe("Shortcuts tab — recording lifecycle", () => {
  // Tier A: a parent re-render (the 1.5 s snapshot poll) with new callback
  // identities must NOT end a recording in progress.
  it("keeps recording across a re-render with new callbacks", async () => {
    const p = setup();
    const r = within(row("Always on Top"));
    await userEvent.click(r.button("Change"));
    p.view.rerender(
      <SettingsView
        {...p}
        settings={{ ...p.settings }}
        onEndRecording={vi.fn()}
        onBeginRecording={vi.fn()}
      />,
    );
    expect(r.keys()).toBe("Press keys…");
    expect(p.onEndRecording).not.toHaveBeenCalled();
  });

  // Tier A: only one row records at a time — the others are locked, so one
  // key press can't be saved to two shortcuts.
  it("locks the other rows while one records", async () => {
    setup();
    await userEvent.click(within(row("Start / Stop Capture")).button("Change"));
    expect(within(row("Always on Top")).button("Change")).toBeDisabled();
    expect(within(row("Show / hide window")).button("Reset")).toBeDisabled();
    fireEvent.keyDown(window, { code: "Escape" });
    expect(within(row("Always on Top")).button("Change")).toBeEnabled();
  });

  // Tier A: the UI ends a recording before the backend's 20 s limit, so it
  // never shows "recording" while shortcuts act again.
  it("ends a recording by itself before the backend limit", async () => {
    vi.useFakeTimers();
    try {
      const p = setup();
      const r = within(row("Always on Top"));
      fireEvent.click(r.button("Change"));
      expect(r.keys()).toBe("Press keys…");
      act(() => vi.advanceTimersByTime(19_000));
      expect(r.keys()).toBe("⌘⇧8");
      expect(p.onEndRecording).toHaveBeenCalledOnce();
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("Shortcuts tab — recording survives polling", () => {
  // Tier A: with the 1.5 s snapshot poll re-rendering (new arrays and
  // callbacks each time), the UI still ends the recording at 19 s — the timer
  // is not restarted by re-renders.
  it("ends at 19 s even while the parent re-renders every 1.5 s", () => {
    vi.useFakeTimers();
    try {
      const p = setup();
      const r = within(row("Always on Top"));
      fireEvent.click(r.button("Change"));
      for (let t = 0; t < 19_000; t += 1_500) {
        p.view.rerender(
          <SettingsView {...p} settings={{ ...p.settings }} onEndRecording={p.onEndRecording} />,
        );
        act(() => vi.advanceTimersByTime(1_500));
      }
      expect(r.keys()).toBe("⌘⇧8");
      expect(p.onEndRecording).toHaveBeenCalledOnce();
      // Subscribed once for the whole recording (no gaps that drop events).
      expect(p.subscribeRecording).toHaveBeenCalledOnce();
    } finally {
      vi.useRealTimers();
    }
  });

  // Tier A: leaving the Shortcuts tab ends the recording (a hidden row must
  // not keep swallowing every key press).
  it("ends the recording when the tab is left", async () => {
    const p = setup();
    await userEvent.click(within(row("Start / Stop Capture")).button("Change"));
    p.view.rerender(<SettingsView {...p} tab="advanced" />);
    expect(p.onEndRecording).toHaveBeenCalledOnce();
    const f = screen.getByLabelText("Packet TTL (hours)");
    fireEvent.focus(f);
    fireEvent.change(f, { target: { value: "36" } });
    expect(f).toHaveValue(36);
  });
});

describe("Advanced tab: updates", () => {
  // Tier A: the automatic update check toggle saves autoCheckUpdates at once.
  it("turns automatic update checks off", async () => {
    const p = setup({ tab: "advanced" });
    expect(screen.getByText("Updates panel")).toBeInTheDocument();
    const box = screen.getByLabelText("Check for updates automatically");
    expect(box).toBeChecked();
    fireEvent.click(box);
    await waitFor(() => expect(p.onPatch).toHaveBeenCalledWith({ autoCheckUpdates: false }));
  });

  // Tier A: where there are no updates (not macOS), there is no setting for them.
  it("hides the automatic check where updates are not available", () => {
    setup({ tab: "advanced", updatesSupported: false });
    expect(screen.queryByLabelText("Check for updates automatically")).toBeNull();
  });
});

describe("Window tab", () => {
  // Tier A: a quick second click toggles back (not the same value twice).
  it("sends the toggled-back value on a quick double click", async () => {
    const p = setup({ tab: "window" });
    let release: (v: string | null) => void = () => {};
    p.onPatch.mockImplementationOnce(() => new Promise((res) => (release = res)));
    const box = screen.getByLabelText("Always on top");
    fireEvent.click(box);
    fireEvent.click(box);
    await waitFor(() => expect(p.onPatch).toHaveBeenCalledTimes(1));
    act(() => release(null));
    await waitFor(() => expect(p.onPatch).toHaveBeenCalledTimes(2));
    expect(p.onPatch.mock.calls[0][0]).toEqual({ alwaysOnTop: true });
    expect(p.onPatch.mock.calls[1][0]).toEqual({ alwaysOnTop: false });
  });

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
    fireEvent.focus(f);
    fireEvent.change(f, { target: { value: "30" } });
    expect(p.onPatch).not.toHaveBeenCalled();
    fireEvent.blur(f);
    await waitFor(() => expect(p.onPatch).toHaveBeenCalledWith({ maxItemBytes: 30 * 1024 * 1024 }));
  });

  // Tier A: an invalid number is not saved; the field goes back.
  it("reverts an invalid number instead of saving it", () => {
    const p = setup({ tab: "advanced" });
    const f = screen.getByLabelText("Packet TTL (hours)");
    fireEvent.focus(f);
    fireEvent.change(f, { target: { value: "0" } });
    fireEvent.blur(f);
    expect(p.onPatch).not.toHaveBeenCalled();
    expect(f).toHaveValue(24);
  });

  // Tier A: when the backend clamps a value (item limit above the packet
  // limit) the field shows what was really saved, not the typed number.
  it("shows the saved value after the backend clamps it", async () => {
    const p = setup({ tab: "advanced", settings: { maxItemBytes: 200 * 1024 * 1024 } });
    const f = screen.getByLabelText("Max item size (MB)");
    fireEvent.focus(f);
    fireEvent.change(f, { target: { value: "500" } });
    fireEvent.blur(f);
    await waitFor(() => expect(p.onPatch).toHaveBeenCalledWith({ maxItemBytes: 500 * 1024 * 1024 }));
    // The saved value is unchanged (clamped to the 200 MB packet limit).
    expect(f).toHaveValue(200);
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

describe("changes still being saved", () => {
  // Tier A: going back to the saved number while the first change is still
  // saving is a real change (the user ends at 24, not 48).
  it("sends a number changed back while the first save is pending", async () => {
    const p = setup({ tab: "advanced" });
    let release: (v: string | null) => void = () => {};
    p.onPatch.mockImplementationOnce(() => new Promise((res) => (release = res)));
    const f = screen.getByLabelText("Packet TTL (hours)");
    fireEvent.focus(f);
    fireEvent.change(f, { target: { value: "48" } });
    fireEvent.blur(f);
    fireEvent.focus(f);
    fireEvent.change(f, { target: { value: "24" } });
    fireEvent.blur(f);
    expect(f).toHaveValue(24);
    await waitFor(() => expect(p.onPatch).toHaveBeenCalledWith({ packetTtlHours: 48 }));
    // Once the first save finishes, the change back to 24 is sent too.
    act(() => release(null));
    await waitFor(() => expect(p.onPatch).toHaveBeenLastCalledWith({ packetTtlHours: 24 }));
  });

  // Tier A: a combination just given to another row (still saving) is
  // refused for this row too.
  it("refuses a combination another row is still saving", async () => {
    const p = setup();
    p.onPatch.mockImplementation(() => new Promise(() => {}));
    await userEvent.click(within(row("Start / Stop Capture")).button("Change"));
    fireEvent.keyDown(window, { code: "KeyK", metaKey: true, altKey: true });
    await waitFor(() =>
      expect(p.onPatch).toHaveBeenCalledWith({ globalShortcut: "CommandOrControl+Alt+K" }),
    );
    await userEvent.click(within(row("Always on Top")).button("Change"));
    fireEvent.keyDown(window, { code: "KeyK", metaKey: true, altKey: true });
    expect(screen.getByRole("alert")).toHaveTextContent("Already used for “Start / Stop Capture”");
    expect(p.onPatch).toHaveBeenCalledTimes(1);
  });
});

