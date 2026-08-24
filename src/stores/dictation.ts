import { create } from "zustand";
import {
  onDictationFinal,
  onDictationState,
  onEngineWarning,
  type DictationFinalEvent,
  type DictationPhase,
} from "../ipc/events";

interface DictationStore {
  phase: DictationPhase;
  profileId: string | null;
  last: DictationFinalEvent | null;
  warning: string | null;
}

export const useDictationStore = create<DictationStore>(() => ({
  phase: "idle",
  profileId: null,
  last: null,
  warning: null,
}));

let attached = false;

/** Subscribe this window to engine events; idempotent per window. */
export function attachDictationEvents() {
  if (attached) return;
  attached = true;
  void onDictationState((e) => {
    useDictationStore.setState({
      phase: e.phase,
      profileId: e.profileId,
      ...(e.phase === "listening" ? { warning: null } : {}),
    });
  });
  void onDictationFinal((e) => {
    useDictationStore.setState({ last: e });
  });
  void onEngineWarning((e) => {
    useDictationStore.setState({ warning: e.message });
  });
}
