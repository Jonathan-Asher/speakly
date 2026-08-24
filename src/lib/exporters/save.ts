import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";

/** Base64-encode without blowing the call stack on large files. */
function toBase64(bytes: Uint8Array): string {
  let binary = "";
  const CHUNK = 0x8000;
  for (let i = 0; i < bytes.length; i += CHUNK) {
    binary += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  }
  return btoa(binary);
}

/**
 * Ask for a destination via the native save dialog, then write the bytes
 * through the `write_binary_file` command (the dialog picked the path, so the
 * user consented to the write). Returns the saved path, or null on cancel.
 */
export async function saveBinary(
  bytes: Uint8Array,
  suggestedName: string,
  extension: string,
  filterName: string,
): Promise<string | null> {
  const path = await save({
    defaultPath: `${suggestedName}.${extension}`,
    filters: [{ name: filterName, extensions: [extension] }],
  });
  if (!path) return null;
  await invoke("write_binary_file", { path, base64Data: toBase64(bytes) });
  return path;
}
