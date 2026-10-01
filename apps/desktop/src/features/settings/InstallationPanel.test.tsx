import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { IntegrationStatus } from "../../lib/api";
import { InstallationPanel } from "./InstallationPanel";

function renderPanel(integrations: IntegrationStatus[]) {
  render(
    <InstallationPanel
      integrations={integrations}
      marketplaceGithub="EarthLinkNetwork/context-drop"
      shortAliasInstalled={false}
      busy={false}
      onInstallIntegration={vi.fn()}
      onInstallShortAlias={vi.fn()}
    />,
  );
}

const notEnabled: IntegrationStatus = {
  configDir: "/Users/u/.claude",
  installed: false,
  enabled: false,
  marketplacePath: "/Users/u/.claude/plugins/marketplaces/context-drop",
};

describe("InstallationPanel", () => {
  it("shows the public install commands when the plugin is not enabled", () => {
    renderPanel([notEnabled]);
    expect(screen.getByTestId("setup-steps")).toBeInTheDocument();
    expect(
      screen.getByText("/plugin marketplace add EarthLinkNetwork/context-drop"),
    ).toBeInTheDocument();
    expect(screen.getByText("/plugin install context-drop@context-drop")).toBeInTheDocument();
    expect(screen.queryByTestId("setup-done")).not.toBeInTheDocument();
  });

  it("shows a done badge when the plugin is installed in Claude Code", () => {
    renderPanel([{ ...notEnabled, enabled: true }]);
    expect(screen.getByTestId("setup-done")).toBeInTheDocument();
    expect(screen.queryByTestId("setup-steps")).not.toBeInTheDocument();
  });
});
