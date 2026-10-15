// Turning a key press into a Tauri global-shortcut accelerator, and showing
// one to the user. Pure (no DOM access) so it can be unit-tested.
//
// Keys come from `KeyboardEvent.code` (the physical key), mapped to the names
// the global-hotkey parser accepts ("KeyA" -> "A", "Digit0" -> "0", ...).
// Modifiers: on macOS Cmd -> "CommandOrControl", Control -> "Control"; on
// Windows/Linux Ctrl -> "CommandOrControl", the Windows key -> "Super".

export type Platform = "mac" | "other";

export interface KeyLike {
  code: string;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}

export type RecordResult =
  /** Only modifiers are held so far: keep listening, show what is held. */
  | { kind: "pending"; preview: string }
  /** Esc without modifiers: stop recording, change nothing. */
  | { kind: "cancel" }
  /** Not usable as a global shortcut: keep listening, show why. */
  | { kind: "invalid"; reason: string; preview: string }
  | { kind: "ok"; accelerator: string };

const MODIFIER_CODES = new Set([
  "MetaLeft",
  "MetaRight",
  "OSLeft",
  "OSRight",
  "ControlLeft",
  "ControlRight",
  "AltLeft",
  "AltRight",
  "ShiftLeft",
  "ShiftRight",
  "CapsLock",
  "Fn",
]);

const NAMED: Record<string, string> = {
  Space: "Space",
  Enter: "Enter",
  NumpadEnter: "Enter",
  Tab: "Tab",
  Backspace: "Backspace",
  Delete: "Delete",
  Home: "Home",
  End: "End",
  PageUp: "PageUp",
  PageDown: "PageDown",
  ArrowUp: "Up",
  ArrowDown: "Down",
  ArrowLeft: "Left",
  ArrowRight: "Right",
  Minus: "Minus",
  Equal: "Equal",
  BracketLeft: "BracketLeft",
  BracketRight: "BracketRight",
  Backslash: "Backslash",
  Semicolon: "Semicolon",
  Quote: "Quote",
  Backquote: "Backquote",
  Comma: "Comma",
  Period: "Period",
  Slash: "Slash",
};

/** The global-hotkey key name for a `KeyboardEvent.code`, or null if unsupported. */
export function keyName(code: string): string | null {
  let m = /^Key([A-Z])$/.exec(code);
  if (m) return m[1];
  m = /^Digit([0-9])$/.exec(code);
  if (m) return m[1];
  m = /^Numpad([0-9])$/.exec(code);
  if (m) return `Num${m[1]}`;
  m = /^F([1-9]|1[0-9]|20)$/.exec(code);
  if (m) return `F${m[1]}`;
  return NAMED[code] ?? null;
}

function modifierParts(e: KeyLike, platform: Platform): string[] {
  const parts: string[] = [];
  if (platform === "mac") {
    if (e.metaKey) parts.push("CommandOrControl");
    if (e.ctrlKey) parts.push("Control");
  } else {
    if (e.ctrlKey) parts.push("CommandOrControl");
    if (e.metaKey) parts.push("Super");
  }
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  return parts;
}

/** Interpret one keydown while recording a shortcut. */
export function recordKey(e: KeyLike, platform: Platform): RecordResult {
  const mods = modifierParts(e, platform);
  const preview = formatAccelerator(mods.join("+"), platform);
  if (MODIFIER_CODES.has(e.code)) return { kind: "pending", preview };
  if (e.code === "Escape" && mods.length === 0) return { kind: "cancel" };
  const key = keyName(e.code);
  const full = formatAccelerator([...mods, key ?? e.code].join("+"), platform);
  if (!key) return { kind: "invalid", reason: "That key can't be used in a shortcut.", preview: full };
  // A global shortcut without Cmd/Ctrl/Option(Alt) would swallow normal typing
  // in every app (Enter, letters, Shift+letter...), so require one of them.
  const strong = e.metaKey || e.ctrlKey || e.altKey;
  if (!strong) {
    const mod = platform === "mac" ? "⌘, ⌃ or ⌥" : "Ctrl or Alt";
    return { kind: "invalid", reason: `Include ${mod} in the combination.`, preview: full };
  }
  return { kind: "ok", accelerator: [...mods, key].join("+") };
}

const MAC_SYMBOL: Record<string, string> = {
  commandorcontrol: "⌘",
  cmdorctrl: "⌘",
  command: "⌘",
  cmd: "⌘",
  super: "⌘",
  control: "⌃",
  ctrl: "⌃",
  alt: "⌥",
  option: "⌥",
  shift: "⇧",
};

const OTHER_NAME: Record<string, string> = {
  commandorcontrol: "Ctrl",
  cmdorctrl: "Ctrl",
  control: "Ctrl",
  ctrl: "Ctrl",
  super: "Win",
  command: "Win",
  cmd: "Win",
  alt: "Alt",
  option: "Alt",
  shift: "Shift",
};

const KEY_LABEL: Record<string, string> = {
  up: "↑",
  down: "↓",
  left: "←",
  right: "→",
  arrowup: "↑",
  arrowdown: "↓",
  arrowleft: "←",
  arrowright: "→",
  minus: "-",
  equal: "=",
  bracketleft: "[",
  bracketright: "]",
  backslash: "\\",
  semicolon: ";",
  quote: "'",
  backquote: "`",
  comma: ",",
  period: ".",
  slash: "/",
};

/** Human-readable form: "⌘⇧9" on macOS, "Ctrl+Shift+9" elsewhere; "" stays "". */
export function formatAccelerator(acc: string, platform: Platform): string {
  if (!acc.trim()) return "";
  const parts = acc.split("+").map((p) => p.trim()).filter(Boolean);
  const out = parts.map((p) => {
    const low = p.toLowerCase();
    const mod = platform === "mac" ? MAC_SYMBOL[low] : OTHER_NAME[low];
    if (mod) return mod;
    return KEY_LABEL[low] ?? p.replace(/^Key([A-Z])$/, "$1").replace(/^Digit([0-9])$/, "$1");
  });
  return platform === "mac" ? out.join("") : out.join("+");
}

/**
 * A canonical form for comparing two accelerators (same key combination ⇒
 * same string): lower-cased, modifier aliases merged, modifiers sorted.
 */
export function normalizeAccelerator(acc: string, platform: Platform): string {
  const parts = acc
    .split("+")
    .map((p) => p.trim().toLowerCase())
    .filter(Boolean);
  const alias = (p: string): string => {
    if (["commandorcontrol", "cmdorctrl"].includes(p)) return platform === "mac" ? "meta" : "ctrl";
    if (["command", "cmd", "super"].includes(p)) return "meta";
    if (["control", "ctrl"].includes(p)) return "ctrl";
    if (["alt", "option"].includes(p)) return "alt";
    if (p === "shift") return "shift";
    return p
      .replace(/^key([a-z])$/, "$1")
      .replace(/^digit([0-9])$/, "$1")
      .replace(/^arrow(up|down|left|right)$/, "$1");
  };
  const mapped = parts.map(alias);
  const mods = mapped.filter((p) => ["meta", "ctrl", "alt", "shift"].includes(p)).sort();
  const keys = mapped.filter((p) => !["meta", "ctrl", "alt", "shift"].includes(p));
  return [...mods, ...keys].join("+");
}

export function detectPlatform(): Platform {
  if (typeof navigator === "undefined") return "mac";
  const p = `${navigator.platform ?? ""} ${navigator.userAgent ?? ""}`;
  return /Mac|iPhone|iPad/i.test(p) ? "mac" : "other";
}
