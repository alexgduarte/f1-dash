/** True when running inside the native app (Tauri webview). */
export const isNative = (): boolean => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
