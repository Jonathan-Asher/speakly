import { create } from "zustand";
import {
  getSettings,
  onSettingsChanged,
  updateSettings,
  type AppSettings,
} from "../ipc/settings";
import { applyTheme } from "../lib/theme";

interface SettingsStore {
  settings: AppSettings | null;
  loaded: boolean;
}

export const useSettingsStore = create<SettingsStore>(() => ({
  settings: null,
  loaded: false,
}));

function apply(settings: AppSettings) {
  // Guard against malformed/empty event payloads: never replace good state
  // with garbage — refetch instead.
  if (!settings || typeof settings !== "object" || !("general" in settings)) {
    void getSettings().then(apply);
    return;
  }
  useSettingsStore.setState({ settings, loaded: true });
  applyTheme(settings.general.theme);
}

let attached = false;

/** Load settings into this window and follow cross-window changes. */
export function attachSettings() {
  if (attached) return;
  attached = true;
  void getSettings().then(apply);
  void onSettingsChanged(apply);
}

/** Deep-merge a patch in Rust; local state updates via the result. */
export async function patchSettings(patch: object) {
  apply(await updateSettings(patch));
}
