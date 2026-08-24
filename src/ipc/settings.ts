import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Theme } from "../lib/theme";

/** The slice of the Rust `Settings` struct the frontend reads/patches.
 * Unknown fields pass through untouched (patches are deep-merged in Rust). */
export interface AppSettings {
  general: {
    launch_at_login: boolean;
    show_dock_icon: boolean;
    theme: Theme;
    sound_feedback: boolean;
  };
  onboarding: { completed: boolean };
  history: {
    enabled: boolean;
    save_dictation: boolean;
    retention_days: number | null;
  };
}

export interface PermissionStatus {
  microphone: "granted" | "denied" | "undetermined" | "unknown";
  accessibility: boolean;
}

export const getSettings = () => invoke<AppSettings>("get_settings_json");

export const updateSettings = (patch: object) =>
  invoke<AppSettings>("update_settings", { patch });

export const checkPermissions = () =>
  invoke<PermissionStatus>("check_permissions");

export const probeMicrophone = () => invoke<void>("probe_microphone");

export const openPrivacyPane = (pane: "microphone" | "accessibility") =>
  invoke<void>("open_privacy_pane", { pane });

export function onSettingsChanged(cb: (s: AppSettings) => void) {
  return listen<AppSettings>("settings://changed", (e) => cb(e.payload));
}
