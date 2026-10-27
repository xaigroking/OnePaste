/** Where the page runs. The shell gives each window a label; a plain browser (the screenshot tests) uses the address. */
export function isQuickPanel(): boolean {
  try {
    if (window.location.hash === "#/quick") return true;
    const internals = (window as unknown as { __TAURI_INTERNALS__?: { metadata?: { currentWindow?: { label?: string } } } }).__TAURI_INTERNALS__;
    return internals?.metadata?.currentWindow?.label === "quick";
  } catch {
    return false;
  }
}
