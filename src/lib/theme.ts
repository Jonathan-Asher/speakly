export type Theme = "system" | "light" | "dark";

const media = window.matchMedia("(prefers-color-scheme: dark)");
let current: Theme = "system";

function render() {
  const dark = current === "dark" || (current === "system" && media.matches);
  document.documentElement.classList.toggle("dark", dark);
}

/** Apply a theme choice; "system" tracks the OS live. */
export function applyTheme(theme: Theme) {
  current = theme;
  render();
}

media.addEventListener("change", () => {
  if (current === "system") render();
});
