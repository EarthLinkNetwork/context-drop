import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { DisplayInfo, Settings } from "../../lib/api";
import { SettingsPanel } from "./SettingsPanel";

const settings: Settings = {
  globalShortcut: "CommandOrControl+Shift+9",
  showWindowShortcut: "CommandOrControl+Alt+D",
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
  { key: "Built-in Retina Display@0,0", name: "Built-in Retina Display", width: 1512, height: 982, primary: true },
  { key: "DELL U2720Q@1512,-200", name: "DELL U2720Q", width: 2560, height: 1440, primary: false },
];

function renderPanel(overrides: Partial<Parameters<typeof SettingsPanel>[0]> = {}) {
  const props = {
    settings,
    shortcutRegistered: true,
    showShortcutRegistered: true,
    busy: false,
    onSave: vi.fn(),
    onOpenDataFolder: vi.fn(),
    onListDisplays: vi.fn().mockResolvedValue(DISPLAYS),
    ...overrides,
  };
  render(<SettingsPanel {...props} />);
  return props;
}

describe("SettingsPanel", () => {
  it("renders current settings values", () => {
    renderPanel();
    expect(screen.getByLabelText("Capture shortcut")).toHaveValue("CommandOrControl+Shift+9");
    expect(screen.getByLabelText("Packet TTL (hours)")).toHaveValue(24);
    expect(screen.getByLabelText("Max item size (MB)")).toHaveValue(25);
    expect(screen.getByLabelText("Max packet size (MB)")).toHaveValue(200);
  });

  it("resyncs the form when the saved settings change (e.g. backend clamp)", () => {
    const { rerender } = render(
      <SettingsPanel
        settings={settings}
        shortcutRegistered
        showShortcutRegistered
        busy={false}
        onSave={vi.fn()}
        onOpenDataFolder={vi.fn()}
        onListDisplays={vi.fn().mockResolvedValue([])}
      />,
    );
    expect(screen.getByLabelText("Packet TTL (hours)")).toHaveValue(24);
    rerender(
      <SettingsPanel
        settings={{ ...settings, packetTtlHours: 1 }}
        shortcutRegistered
        showShortcutRegistered
        busy={false}
        onSave={vi.fn()}
        onOpenDataFolder={vi.fn()}
        onListDisplays={vi.fn().mockResolvedValue([])}
      />,
    );
    expect(screen.getByLabelText("Packet TTL (hours)")).toHaveValue(1);
  });

  it("shows the shortcut-conflict warning only when registration failed", () => {
    const { rerender } = render(
      <SettingsPanel
        settings={settings}
        shortcutRegistered={true}
        showShortcutRegistered
        busy={false}
        onSave={vi.fn()}
        onOpenDataFolder={vi.fn()}
        onListDisplays={vi.fn().mockResolvedValue([])}
      />,
    );
    expect(screen.queryByTestId("shortcut-conflict")).not.toBeInTheDocument();
    rerender(
      <SettingsPanel
        settings={settings}
        shortcutRegistered={false}
        showShortcutRegistered
        busy={false}
        onSave={vi.fn()}
        onOpenDataFolder={vi.fn()}
        onListDisplays={vi.fn().mockResolvedValue([])}
      />,
    );
    expect(screen.getByTestId("shortcut-conflict")).toBeInTheDocument();
  });

  it("saves settings, converting MB inputs back to bytes", async () => {
    const onSave = vi.fn();
    renderPanel({ onSave });
    fireEvent.change(screen.getByLabelText("Max item size (MB)"), { target: { value: "30" } });
    fireEvent.change(screen.getByLabelText("Packet TTL (hours)"), { target: { value: "48" } });
    await userEvent.click(screen.getByRole("button", { name: "Save Settings" }));
    expect(onSave).toHaveBeenCalledOnce();
    const saved = onSave.mock.calls[0][0] as Settings;
    expect(saved.maxItemBytes).toBe(30 * 1024 * 1024);
    expect(saved.packetTtlHours).toBe(48);
  });

  // Tier A: every new window setting must reach onSave (a dropped field
  // would silently keep the old behaviour after "Save Settings").
  it("saves the window settings", async () => {
    const onSave = vi.fn();
    renderPanel({ onSave });
    fireEvent.change(screen.getByLabelText("Show window shortcut"), {
      target: { value: "  CommandOrControl+Alt+J " },
    });
    await userEvent.selectOptions(screen.getByLabelText("Open window on"), "display");
    const picker = await screen.findByLabelText("Display");
    await screen.findByRole("option", { name: /DELL U2720Q/ });
    await userEvent.selectOptions(picker, "DELL U2720Q@1512,-200");
    await userEvent.click(screen.getByLabelText("Always on top"));
    await userEvent.click(screen.getByLabelText("Show on all desktops"));
    await userEvent.click(screen.getByRole("button", { name: "Save Settings" }));
    const saved = onSave.mock.calls[0][0] as Settings;
    expect(saved.showWindowShortcut).toBe("CommandOrControl+Alt+J");
    expect(saved.windowPlacement).toBe("display");
    expect(saved.windowDisplay).toBe("DELL U2720Q@1512,-200");
    expect(saved.alwaysOnTop).toBe(true);
    expect(saved.showOnAllDesktops).toBe(true);
  });

  // Tier A: clearing the show-window shortcut turns it off; it must not be
  // replaced by the old value (as the capture shortcut is).
  it("saves an empty show-window shortcut as off", async () => {
    const onSave = vi.fn();
    renderPanel({ onSave });
    fireEvent.change(screen.getByLabelText("Show window shortcut"), { target: { value: "" } });
    await userEvent.click(screen.getByRole("button", { name: "Save Settings" }));
    expect((onSave.mock.calls[0][0] as Settings).showWindowShortcut).toBe("");
  });

  // Tier A: the display picker only exists for "A specific display".
  it("shows the display picker only for a specific display", async () => {
    const onListDisplays = vi.fn().mockResolvedValue(DISPLAYS);
    renderPanel({ onListDisplays });
    expect(screen.queryByLabelText("Display")).not.toBeInTheDocument();
    expect(onListDisplays).not.toHaveBeenCalled();
    await userEvent.selectOptions(screen.getByLabelText("Open window on"), "display");
    expect(await screen.findByRole("option", { name: /Built-in Retina Display .* — main/ })).toBeInTheDocument();
    expect(onListDisplays).toHaveBeenCalledOnce();
  });

  // Tier A: a saved display that is unplugged is labelled, not silently
  // swapped for another one.
  it("marks a saved display that is not connected", async () => {
    renderPanel({
      settings: { ...settings, windowPlacement: "display", windowDisplay: "LG UltraFine@-1920,0" },
    });
    expect(await screen.findByRole("option", { name: "LG UltraFine (not connected)" })).toBeInTheDocument();
    expect(screen.getByTestId("display-missing")).toBeInTheDocument();
  });

  // Tier A: a rearranged display (same name, new origin) is matched by name,
  // as the backend does, and saving stores its current key.
  it("matches a rearranged display by name", async () => {
    const onSave = vi.fn();
    renderPanel({
      onSave,
      settings: { ...settings, windowPlacement: "display", windowDisplay: "DELL U2720Q@-2560,0" },
    });
    await screen.findByRole("option", { name: /DELL U2720Q \(2560×1440\)/ });
    expect(screen.queryByTestId("display-missing")).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Save Settings" }));
    expect((onSave.mock.calls[0][0] as Settings).windowDisplay).toBe("DELL U2720Q@1512,-200");
  });

  // Tier A: the show-window conflict warning follows the snapshot flag, and
  // is not shown when the shortcut is simply off.
  it("warns when the show-window shortcut failed to register", () => {
    const { rerender } = render(
      <SettingsPanel
        settings={settings}
        shortcutRegistered
        showShortcutRegistered={false}
        busy={false}
        onSave={vi.fn()}
        onOpenDataFolder={vi.fn()}
        onListDisplays={vi.fn().mockResolvedValue([])}
      />,
    );
    expect(screen.getByTestId("show-shortcut-conflict")).toBeInTheDocument();
    rerender(
      <SettingsPanel
        settings={{ ...settings, showWindowShortcut: "" }}
        shortcutRegistered
        showShortcutRegistered={false}
        busy={false}
        onSave={vi.fn()}
        onOpenDataFolder={vi.fn()}
        onListDisplays={vi.fn().mockResolvedValue([])}
      />,
    );
    expect(screen.queryByTestId("show-shortcut-conflict")).not.toBeInTheDocument();
  });

  // Tier A: a tray "Always on Top" toggle changes only that saved value; it
  // must not wipe the user's unsaved edits in other fields.
  it("keeps unsaved edits when another saved value changes", () => {
    const props = {
      shortcutRegistered: true,
      showShortcutRegistered: true,
      busy: false,
      onSave: vi.fn(),
      onOpenDataFolder: vi.fn(),
      onListDisplays: vi.fn().mockResolvedValue([]),
    };
    const { rerender } = render(<SettingsPanel settings={settings} {...props} />);
    fireEvent.change(screen.getByLabelText("Packet TTL (hours)"), { target: { value: "48" } });
    fireEvent.change(screen.getByLabelText("Capture shortcut"), {
      target: { value: "CommandOrControl+Alt+K" },
    });
    rerender(<SettingsPanel settings={{ ...settings, alwaysOnTop: true }} {...props} />);
    expect(screen.getByLabelText("Always on top")).toBeChecked();
    expect(screen.getByLabelText("Packet TTL (hours)")).toHaveValue(48);
    expect(screen.getByLabelText("Capture shortcut")).toHaveValue("CommandOrControl+Alt+K");
  });
});
