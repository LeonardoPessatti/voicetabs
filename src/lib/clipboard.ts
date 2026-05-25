/**
 * Thin wrapper around `navigator.clipboard.writeText` so callers and tests
 * can mock a single export instead of poking the global navigator. In
 * Tauri's WebView2 the Clipboard API is available without explicit permission
 * prompts; we still fall back to a no-op + console.warn if it's missing
 * (older WebView2 runtimes or jsdom without polyfill).
 */
export async function writeText(text: string): Promise<void> {
  if (navigator.clipboard && typeof navigator.clipboard.writeText === "function") {
    await navigator.clipboard.writeText(text);
    return;
  }
  console.warn("clipboard.writeText: navigator.clipboard unavailable");
}
