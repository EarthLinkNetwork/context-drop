import { useState } from "react";
import type { IntegrationStatus } from "../../lib/api";

interface Props {
  integrations: IntegrationStatus[];
  marketplaceGithub: string;
  shortAliasInstalled: boolean;
  busy: boolean;
  onInstallIntegration: (configDir: string) => void;
  onInstallShortAlias: () => void;
}

/** A command line with a copy button. */
function CopyRow({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  const copy = () => {
    void (async () => {
      try {
        await navigator.clipboard?.writeText(text);
        setCopied(true);
        setTimeout(() => setCopied(false), 1200);
      } catch {
        /* clipboard unavailable (e.g. tests) — ignore */
      }
    })();
  };
  return (
    <div className="copy-row">
      <code>{text}</code>
      <button type="button" className="btn btn-small copy-btn" onClick={copy}>
        {copied ? "Copied" : "Copy"}
      </button>
    </div>
  );
}

/**
 * The "Claude Code Setup" section: a clear step-by-step install flow with
 * copyable commands and a live status badge (detected from Claude Code's own
 * installed-plugins record), plus an optional/offline path.
 */
export function InstallationPanel({
  integrations,
  marketplaceGithub,
  shortAliasInstalled,
  busy,
  onInstallIntegration,
  onInstallShortAlias,
}: Props) {
  const enabled = integrations.some((i) => i.enabled);

  return (
    <section className="panel installation-panel">
      <h2>Claude Code Setup</h2>

      {enabled ? (
        <p className="installed" data-testid="setup-done">
          ✓ Plugin installed in Claude Code — you're ready. Run{" "}
          <code>/context-drop:pull &lt;instruction&gt;</code> in any session (or <code>/cd</code>{" "}
          once the optional short alias below is installed).
        </p>
      ) : (
        <ol className="setup-steps" data-testid="setup-steps">
          <li>
            <strong>Step 1 — Enable the plugin in Claude Code.</strong> Paste these into a Claude
            Code session, then open a new session:
            <CopyRow text={`/plugin marketplace add ${marketplaceGithub}`} />
            <CopyRow text="/plugin install context-drop@context-drop" />
          </li>
          <li>
            <strong>Step 2 — Capture &amp; route.</strong> Press Start Capture (or drag &amp; drop
            files), then run <code>/context-drop:pull &lt;instruction&gt;</code> in the target tab
            (or <code>/cd</code> once the optional short alias below is installed — the plugin
            itself does not include <code>/cd</code>).
          </li>
        </ol>
      )}

      <details className="optional-setup">
        <summary>Optional / offline install</summary>
        <div className="optional-body">
          <button
            type="button"
            className="btn btn-small"
            disabled={busy || shortAliasInstalled}
            onClick={onInstallShortAlias}
          >
            {shortAliasInstalled ? "/cd alias installed" : "Install /cd short alias"}
          </button>
          <p className="hint">
            No internet? Stage a local marketplace instead of adding from GitHub, then add its path:
          </p>
          <table className="integrations" aria-label="Claude config directories">
            <tbody>
              {integrations.length === 0 && (
                <tr>
                  <td className="empty">No Claude config directories detected.</td>
                </tr>
              )}
              {integrations.map((it) => (
                <tr key={it.configDir}>
                  <td className="config-path">{it.configDir}</td>
                  <td className="config-status">
                    {it.enabled ? (
                      <span className="installed">✓ installed</span>
                    ) : it.installed ? (
                      <span className="hint">staged</span>
                    ) : (
                      <button
                        type="button"
                        className="btn btn-small"
                        disabled={busy}
                        onClick={() => onInstallIntegration(it.configDir)}
                      >
                        Install locally
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {integrations.some((it) => it.installed && !it.enabled) && (
            <>
              <p className="hint">Then add the staged marketplace:</p>
              {integrations
                .filter((it) => it.installed && !it.enabled)
                .map((it) => (
                  <CopyRow key={it.configDir} text={`/plugin marketplace add ${it.marketplacePath}`} />
                ))}
              <CopyRow text="/plugin install context-drop@context-drop" />
            </>
          )}
        </div>
      </details>
    </section>
  );
}
