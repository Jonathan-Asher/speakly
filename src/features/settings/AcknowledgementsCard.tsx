import { openUrl } from "@tauri-apps/plugin-opener";

const ITEMS: { name: string; license: string; url: string }[] = [
  { name: "whisper.cpp", license: "MIT", url: "https://github.com/ggml-org/whisper.cpp" },
  { name: "ggml", license: "MIT", url: "https://github.com/ggml-org/ggml" },
  {
    name: "ivrit.ai Hebrew models",
    license: "Apache-2.0",
    url: "https://huggingface.co/ivrit-ai",
  },
  { name: "Silero VAD", license: "MIT", url: "https://github.com/snakers4/silero-vad" },
  {
    name: "sherpa-onnx (planned)",
    license: "Apache-2.0",
    url: "https://github.com/k2-fsa/sherpa-onnx",
  },
  { name: "Tauri", license: "MIT / Apache-2.0", url: "https://tauri.app" },
  { name: "React", license: "MIT", url: "https://react.dev" },
];

/** The one place open-source components are credited. */
export function AcknowledgementsCard() {
  return (
    <div className="rounded-xl border border-neutral-200 p-4 dark:border-neutral-800">
      <div className="font-medium">Acknowledgements</div>
      <div className="mt-1 text-xs text-neutral-500">
        Speakly is built on excellent open-source work.
      </div>
      <ul className="mt-3 space-y-1.5">
        {ITEMS.map((item) => (
          <li key={item.name} className="flex items-baseline justify-between text-sm">
            <button
              className="text-start text-accent-strong hover:underline dark:text-accent"
              onClick={() => void openUrl(item.url)}
            >
              {item.name}
            </button>
            <span className="text-xs text-neutral-500">{item.license}</span>
          </li>
        ))}
      </ul>
      <div className="mt-3 text-xs text-neutral-500">
        …and other open-source Rust and JavaScript dependencies.
      </div>
    </div>
  );
}
