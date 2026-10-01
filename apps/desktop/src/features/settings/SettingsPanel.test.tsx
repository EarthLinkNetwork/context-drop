import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { Settings } from "../../lib/api";
import { SettingsPanel } from "./SettingsPanel";

const settings: Settings = {
  globalShortcut: "CommandOrControl+Shift+9",
  packetTtlHours: 24,
  maxItemBytes: 25 * 1024 * 1024,
  maxPacketBytes: 200 * 1024 * 1024,
  autoCleanup: true,
  shortAliasInstalled: false,
};

function renderPanel(overrides: Partial<Parameters<typeof SettingsPanel>[0]> = {}) {
  const props = {
    settings,
    shortcutRegistered: true,
    integrations: [
      { configDir: "/Users/u/.claude", installed: true },
      { configDir: "/Users/u/.claude-work", installed: false },
    ],
    busy: false,
    onSave: vi.fn(),
    onInstallIntegration: vi.fn(),
    onInstallShortAlias: vi.fn(),
    onOpenDataFolder: vi.fn(),
    ...overrides,
  };
  render(<SettingsPanel {...props} />);
  return props;
}

describe("SettingsPanel", () => {
  it("renders current settings values", () => {
    renderPanel();
    expect(screen.getByLabelText("Global shortcut")).toHaveValue("CommandOrControl+Shift+9");
    expect(screen.getByLabelText("Packet TTL (hours)")).toHaveValue(24);
    expect(screen.getByLabelText("Max item size (MB)")).toHaveValue(25);
    expect(screen.getByLabelText("Max packet size (MB)")).toHaveValue(200);
  });

  it("resyncs the form when the saved settings change (e.g. backend clamp)", () => {
    const base = { ...settings };
    const { rerender } = render(
      <SettingsPanel
        settings={base}
        shortcutRegistered
        integrations={[]}
        busy={false}
        onSave={vi.fn()}
        onInstallIntegration={vi.fn()}
        onInstallShortAlias={vi.fn()}
        onOpenDataFolder={vi.fn()}
      />,
    );
    expect(screen.getByLabelText("Packet TTL (hours)")).toHaveValue(24);
    // Backend saved a clamped value; the new snapshot flows in as props.
    rerender(
      <SettingsPanel
        settings={{ ...base, packetTtlHours: 1 }}
        shortcutRegistered
        integrations={[]}
        busy={false}
        onSave={vi.fn()}
        onInstallIntegration={vi.fn()}
        onInstallShortAlias={vi.fn()}
        onOpenDataFolder={vi.fn()}
      />,
    );
    expect(screen.getByLabelText("Packet TTL (hours)")).toHaveValue(1);
  });

  it("shows the shortcut-conflict warning only when registration failed", () => {
    const { rerender } = render(
      <SettingsPanel
        settings={settings}
        shortcutRegistered={true}
        integrations={[]}
        busy={false}
        onSave={vi.fn()}
        onInstallIntegration={vi.fn()}
        onInstallShortAlias={vi.fn()}
        onOpenDataFolder={vi.fn()}
      />,
    );
    expect(screen.queryByTestId("shortcut-conflict")).not.toBeInTheDocument();
    rerender(
      <SettingsPanel
        settings={settings}
        shortcutRegistered={false}
        integrations={[]}
        busy={false}
        onSave={vi.fn()}
        onInstallIntegration={vi.fn()}
        onInstallShortAlias={vi.fn()}
        onOpenDataFolder={vi.fn()}
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

  it("lists integration status and installs into a config dir", async () => {
    const onInstallIntegration = vi.fn();
    renderPanel({ onInstallIntegration });
    expect(screen.getByText("/Users/u/.claude")).toBeInTheDocument();
    expect(screen.getByText("Installed")).toBeInTheDocument();
    // The not-installed row offers an Install button.
    const installBtn = screen.getByRole("button", { name: "Install" });
    await userEvent.click(installBtn);
    expect(onInstallIntegration).toHaveBeenCalledWith("/Users/u/.claude-work");
  });

  it("reflects short-alias installed state", () => {
    renderPanel({ settings: { ...settings, shortAliasInstalled: true } });
    expect(screen.getByRole("button", { name: "/cd alias installed" })).toBeDisabled();
  });
});
