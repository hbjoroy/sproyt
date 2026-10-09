/** Grow to 3½ text lines; keep longer drafts inside the composer. */
export function resizeComposer(field: HTMLTextAreaElement) {
  const style = getComputedStyle(field);
  const line = parseFloat(style.lineHeight) || 24;
  const padding = (parseFloat(style.paddingTop) || 0) + (parseFloat(style.paddingBottom) || 0);
  const border = (parseFloat(style.borderTopWidth) || 0) + (parseFloat(style.borderBottomWidth) || 0);
  field.style.height = 'auto';
  field.style.height = `${Math.max(parseFloat(style.minHeight) || 44, Math.min(field.scrollHeight + border, line * 3.5 + padding + border))}px`;
}
export function observeComposer(field: HTMLTextAreaElement) {
  resizeComposer(field);
  if (typeof ResizeObserver === 'undefined') return () => {};
  let width = field.clientWidth;
  const observer = new ResizeObserver(() => {
    if (field.clientWidth !== width) { width = field.clientWidth; resizeComposer(field); }
  });
  observer.observe(field);
  return () => observer.disconnect();
}
