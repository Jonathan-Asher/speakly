/**
 * All user-facing strings live here so Hebrew UI localization stays cheap
 * later. App chrome is LTR English for now; transcript content is always
 * direction-aware via <DirectionalText>.
 */
export const strings = {
  appName: "Speakly",
  tagline: "Speech to text, instantly.",
  nav: {
    dictation: "Dictation",
    files: "Files",
    meetings: "Meetings",
    history: "History",
    models: "Models",
    settings: "Settings",
  },
  hud: {
    listening: "Listening",
    transcribing: "Transcribing…",
    translating: "Translating…",
    pasted: "Pasted",
  },
} as const;
