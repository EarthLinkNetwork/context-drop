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

// The key names global-hotkey 0.8.0's `parse_key` accepts, each folded to one
// canonical name (its Code). Mirrors that table so the UI's duplicate check
// agrees with the backend's `Shortcut::id()` for every spelling.
const KEY_CANON: Record<string, string> = (() => {
  const t: Record<string, string> = {};
  const add = (canon: string, ...names: string[]) => {
    for (const n of [canon, ...names]) t[n.toUpperCase()] = canon;
  };
  add("Backquote", "`");
  add("Backslash", "\\");
  add("BracketLeft", "[");
  add("BracketRight", "]");
  add("Pause", "PauseBreak");
  add("Comma", ",");
  for (let i = 0; i <= 9; i++) add(`Digit${i}`, `${i}`);
  add("Equal", "=");
  for (const c of "ABCDEFGHIJKLMNOPQRSTUVWXYZ") add(`Key${c}`, c);
  add("Minus", "-");
  add("Period", ".");
  add("Quote", "'");
  add("Semicolon", ";");
  add("Slash", "/");
  for (const k of ["Backspace", "CapsLock", "Enter", "Space", "Tab", "Delete", "End", "Home"])
    add(k);
  for (const k of ["Insert", "PageDown", "PageUp", "PrintScreen", "ScrollLock", "NumLock"]) add(k);
  add("ArrowDown", "Down");
  add("ArrowLeft", "Left");
  add("ArrowRight", "Right");
  add("ArrowUp", "Up");
  for (let i = 0; i <= 9; i++) add(`Numpad${i}`, `Num${i}`);
  add("NumpadAdd", "NumAdd", "NumpadPlus", "NumPlus");
  add("NumpadDecimal", "NumDecimal");
  add("NumpadDivide", "NumDivide");
  add("NumpadEnter", "NumEnter");
  add("NumpadEqual", "NumEqual");
  add("NumpadMultiply", "NumMultiply");
  add("NumpadSubtract", "NumSubtract");
  add("Escape", "Esc");
  for (let i = 1; i <= 24; i++) add(`F${i}`);
  add("AudioVolumeDown", "VolumeDown");
  add("AudioVolumeUp", "VolumeUp");
  add("AudioVolumeMute", "VolumeMute");
  for (const k of ["MediaPlay", "MediaPause", "MediaPlayPause", "MediaStop", "MediaTrackNext"])
    add(k);
  add("MediaTrackPrevious", "MediaTrackPrev");
  return t;
})();

/** Canonical key (its Code) for any spelling global-hotkey parses, or null. */
export function canonicalKey(name: string): string | null {
  return KEY_CANON[name.trim().toUpperCase()] ?? null;
}

// Parseable, but not usable as a recorded global shortcut here: no macOS
// keycode for them in global-hotkey (F21+, Pause, ScrollLock, NumLock, media
// keys) or they act as modifiers (CapsLock).
const NOT_RECORDABLE = new Set([
  "F21",
  "F22",
  "F23",
  "F24",
  "Pause",
  "ScrollLock",
  "NumLock",
  "CapsLock",
  "PrintScreen",
  "Insert",
  "AudioVolumeDown",
  "AudioVolumeUp",
  "AudioVolumeMute",
  "MediaPlay",
  "MediaPause",
  "MediaPlayPause",
  "MediaStop",
  "MediaTrackNext",
  "MediaTrackPrevious",
]);

/**
 * The accelerator key name for a `KeyboardEvent.code`, or null if it can't be
 * used. Short, readable forms for letters, digits and arrows ("A", "0", "Up");
 * every other key keeps its Code name, which global-hotkey parses directly.
 */
export function keyName(code: string): string | null {
  const canon = canonicalKey(code);
  if (!canon || canon !== code || NOT_RECORDABLE.has(canon)) return null;
  let m = /^Key([A-Z])$/.exec(canon);
  if (m) return m[1];
  m = /^Digit([0-9])$/.exec(canon);
  if (m) return m[1];
  m = /^Arrow(Up|Down|Left|Right)$/.exec(canon);
  if (m) return m[1];
  return canon;
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
  // (The Windows key does not count on Windows/Linux: the OS owns most of its
  // combinations.)
  const strong = platform === "mac" ? e.metaKey || e.ctrlKey || e.altKey : e.ctrlKey || e.altKey;
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
  escape: "Esc",
  numpadadd: "Num +",
  numpadsubtract: "Num −",
  numpadmultiply: "Num ×",
  numpaddivide: "Num ÷",
  numpaddecimal: "Num .",
  numpadequal: "Num =",
  numpadenter: "Num Enter",
  numenter: "Num Enter",
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
 * same string), following global-hotkey's `parse_hotkey`: modifier aliases
 * (Option/Alt, Control/Ctrl, Command/Cmd/Super, the CommandOrControl family
 * — Super on macOS, Control elsewhere) and key aliases are merged, modifiers
 * sorted.
 */
export function normalizeAccelerator(acc: string, platform: Platform): string {
  const mods = new Set<string>();
  const keys: string[] = [];
  for (const raw of acc.split("+")) {
    const p = raw.trim().toUpperCase();
    if (!p) continue;
    if (p === "OPTION" || p === "ALT") mods.add("alt");
    else if (p === "CONTROL" || p === "CTRL") mods.add("ctrl");
    else if (p === "COMMAND" || p === "CMD" || p === "SUPER") mods.add("meta");
    else if (p === "SHIFT") mods.add("shift");
    else if (["COMMANDORCONTROL", "COMMANDORCTRL", "CMDORCTRL", "CMDORCONTROL"].includes(p))
      mods.add(platform === "mac" ? "meta" : "ctrl");
    else keys.push(canonicalKey(p) ?? p.toLowerCase());
  }
  return [...[...mods].sort(), ...keys].join("+");
}

export function detectPlatform(): Platform {
  if (typeof navigator === "undefined") return "mac";
  const p = `${navigator.platform ?? ""} ${navigator.userAgent ?? ""}`;
  return /Mac|iPhone|iPad/i.test(p) ? "mac" : "other";
}
