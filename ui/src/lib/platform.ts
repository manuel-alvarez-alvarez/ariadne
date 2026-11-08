/**
 * Marks `<html>` with the platform where the app must look different.
 *
 * WebKitGTK draws fat, system-coloured scrollbars that sit on top of content;
 * `index.css` restyles them, but only under `data-platform="linux"`, so the
 * overlay scrollbars of macOS stay as they are.
 */
export function markPlatform(root: HTMLElement, userAgent: string): void {
  if (/Linux/.test(userAgent) && !/Android/.test(userAgent)) {
    root.dataset.platform = "linux"
  }
}
