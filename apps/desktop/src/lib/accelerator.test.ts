import { describe, expect, it } from "vitest";
import {
  formatAccelerator,
  keyName,
  normalizeAccelerator,
  recordKey,
  type KeyLike,
} from "./accelerator";

function key(code: string, mods: Partial<KeyLike> = {}): KeyLike {
  return { code, metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, ...mods };
}

describe("recordKey", () => {
  // Tier A: the key combination pressed becomes the accelerator Tauri parses
  // (Cmd → CommandOrControl on macOS), so no one has to type it by hand.
  it("records Cmd+Shift+9 on macOS as CommandOrControl+Shift+9", () => {
    expect(recordKey(key("Digit9", { metaKey: true, shiftKey: true }), "mac")).toEqual({
      kind: "ok",
      accelerator: "CommandOrControl+Shift+9",
    });
  });

  // Tier A: on macOS Control is its own modifier, not CommandOrControl.
  it("keeps Control distinct from Cmd on macOS", () => {
    expect(recordKey(key("KeyK", { ctrlKey: true, altKey: true }), "mac")).toEqual({
      kind: "ok",
      accelerator: "Control+Alt+K",
    });
  });

  // Tier A: on Windows/Linux Ctrl is CommandOrControl and the Win key is Super.
  it("maps Ctrl and Win on other platforms", () => {
    expect(recordKey(key("KeyD", { ctrlKey: true, metaKey: true }), "other")).toEqual({
      kind: "ok",
      accelerator: "CommandOrControl+Super+D",
    });
  });

  // Tier A: Enter alone (or any key without Cmd/Ctrl/Option) can never become
  // a global shortcut — it would swallow normal typing in every app.
  it("refuses Enter, letters and Shift-only combinations", () => {
    for (const k of [key("Enter"), key("KeyA"), key("KeyA", { shiftKey: true }), key("Tab")]) {
      const r = recordKey(k, "mac");
      expect(r.kind, k.code).toBe("invalid");
    }
  });

  // Tier A: Esc (no modifiers) cancels; Esc with a modifier is a normal key.
  it("treats plain Esc as cancel", () => {
    expect(recordKey(key("Escape"), "mac")).toEqual({ kind: "cancel" });
  });

  // Tier A: holding only modifiers keeps waiting and previews them.
  it("waits while only modifiers are held", () => {
    expect(recordKey(key("MetaLeft", { metaKey: true, shiftKey: true }), "mac")).toEqual({
      kind: "pending",
      preview: "⌘⇧",
    });
  });

  // Tier A: keys global-hotkey can't parse are refused, not saved broken.
  it("refuses keys global-hotkey cannot parse", () => {
    expect(recordKey(key("IntlYen", { metaKey: true }), "mac").kind).toBe("invalid");
    expect(recordKey(key("Lang1", { metaKey: true }), "mac").kind).toBe("invalid");
  });
});

describe("keyName", () => {
  // Tier A: names match what global-hotkey's parser accepts.
  it("maps codes to parser names", () => {
    expect(keyName("KeyA")).toBe("A");
    expect(keyName("Digit0")).toBe("0");
    expect(keyName("F12")).toBe("F12");
    expect(keyName("F21")).toBeNull();
    expect(keyName("ArrowUp")).toBe("Up");
    expect(keyName("Space")).toBe("Space");
    expect(keyName("Pause")).toBeNull();
    // The keypad Enter is its own key (not Return).
    expect(keyName("NumpadEnter")).toBe("NumpadEnter");
  });
});

describe("formatAccelerator", () => {
  it("shows macOS symbols and other-platform names", () => {
    expect(formatAccelerator("CommandOrControl+Shift+9", "mac")).toBe("⌘⇧9");
    expect(formatAccelerator("Control+Alt+K", "mac")).toBe("⌃⌥K");
    expect(formatAccelerator("CommandOrControl+Shift+9", "other")).toBe("Ctrl+Shift+9");
    expect(formatAccelerator("", "mac")).toBe("");
  });
});

describe("normalizeAccelerator", () => {
  // Tier A: the same combination spelled differently is detected as the same
  // (an older hand-typed "Cmd+Shift+9" vs a recorded CommandOrControl one).
  it("treats aliases and modifier order as the same combination", () => {
    const a = normalizeAccelerator("CommandOrControl+Shift+9", "mac");
    expect(normalizeAccelerator("shift+cmd+9", "mac")).toBe(a);
    expect(normalizeAccelerator("Cmd+Shift+Digit9", "mac")).toBe(a);
    expect(normalizeAccelerator("Control+Shift+9", "mac")).not.toBe(a);
    // Key spellings global-hotkey treats as one key (a hand-typed v0.1.8 value).
    expect(normalizeAccelerator("CmdOrCtrl+Shift+-", "mac")).toBe(
      normalizeAccelerator("CommandOrControl+Shift+Minus", "mac"),
    );
    expect(normalizeAccelerator("Cmd+Alt+Num1", "mac")).toBe(
      normalizeAccelerator("Cmd+Alt+Numpad1", "mac"),
    );
    expect(normalizeAccelerator("Ctrl+Esc", "mac")).toBe(normalizeAccelerator("Ctrl+Escape", "mac"));
    expect(normalizeAccelerator("Ctrl+Shift+9", "other")).toBe(
      normalizeAccelerator("CommandOrControl+Shift+9", "other"),
    );
  });
});
