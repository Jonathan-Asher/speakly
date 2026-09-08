/** Accelerator-string helpers shared by the profile UI and the popover. */

/** Bare-modifier hold specs (handled by an event tap, not the plugin). */
const BARE_PRETTY: Record<string, string> = {
  RightOption: "⌥ right",
  LeftOption: "⌥ left",
  RightCommand: "⌘ right",
  Fn: "fn",
};

export function isBareModifier(hotkey: string) {
  return hotkey in BARE_PRETTY;
}

/** "RightOption+KeyM" → ["RightOption", "KeyM"], else null. */
export function splitSided(hotkey: string): [string, string] | null {
  const at = hotkey.indexOf("+");
  if (at < 0) return null;
  const modifier = hotkey.slice(0, at);
  const key = hotkey.slice(at + 1);
  return modifier in BARE_PRETTY && key ? [modifier, key] : null;
}

/** A key name (`KeyM`, `Space`, `Digit1`) as shown to the user. */
function prettyKey(key: string) {
  return key
    .replace(/^Key/, "")
    .replace(/^Digit/, "")
    .replace(/^Arrow/, "")
    .replace(/^Space$/, "Space");
}

export function prettyHotkey(hotkey: string) {
  const bare = BARE_PRETTY[hotkey];
  if (bare) return bare;
  // Side-specific combination — must be handled before the generic
  // substitutions below, which would mangle "Right"/"Left" inside the name.
  const sided = splitSided(hotkey);
  if (sided) return `${BARE_PRETTY[sided[0]]} + ${prettyKey(sided[1])}`;
  return hotkey
    .replace(/Alt/g, "⌥")
    .replace(/Shift/g, "⇧")
    .replace(/(CommandOrControl|Command|Super)/g, "⌘")
    .replace(/Control|Ctrl/g, "⌃")
    .replace(/Up/g, "↑")
    .replace(/Down/g, "↓")
    .replace(/Left/g, "←")
    .replace(/Right/g, "→")
    .replace(/\+/g, " ");
}

const MODIFIER_KEYS = new Set(["Shift", "Control", "Alt", "Meta"]);

/**
 * Map a modifier-only keydown to a bare-hold spec ("RightOption" etc.), or
 * null when the pressed modifier isn't one we support alone. The recorder
 * offers this only when the modifier is pressed and released with no other
 * key in between.
 */
export function bareModifierFromEvent(e: KeyboardEvent): string | null {
  switch (e.code) {
    case "AltRight":
      return "RightOption";
    case "AltLeft":
      return "LeftOption";
    case "MetaRight":
      return "RightCommand";
    default:
      return null;
  }
}

/**
 * Map a KeyboardEvent to the plugin's accelerator syntax
 * (e.g. "Ctrl+Alt+Space", "Alt+D", "F6"), or null when the event can't
 * finish a recording yet (bare modifier, unsupported key, or a combo
 * without any modifier on a non-F-key).
 */
export function eventToAccelerator(e: KeyboardEvent): string | null {
  if (MODIFIER_KEYS.has(e.key)) return null;

  let key: string | null = null;
  const code = e.code;
  if (/^Key[A-Z]$/.test(code)) key = code.slice(3);
  else if (/^Digit[0-9]$/.test(code)) key = code.slice(5);
  else if (code === "Space") key = "Space";
  else if (/^F([1-9]|1[0-2])$/.test(code)) key = code;
  else if (code === "ArrowUp") key = "Up";
  else if (code === "ArrowDown") key = "Down";
  else if (code === "ArrowLeft") key = "Left";
  else if (code === "ArrowRight") key = "Right";
  if (!key) return null;

  const mods: string[] = [];
  if (e.ctrlKey) mods.push("Ctrl");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  if (e.metaKey) mods.push("Super");

  // A hotkey with no modifier would swallow normal typing; only bare
  // function keys are allowed unmodified.
  if (mods.length === 0 && !/^F/.test(key)) return null;

  return [...mods, key].join("+");
}
