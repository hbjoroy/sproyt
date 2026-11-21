import type { KeyboardEvent } from "react";

/** Arrow keys follow the tabs actually offered in this scope. */
export function navigateTabList(event: KeyboardEvent<HTMLElement>) {
  const tabs = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>('button[role="tab"]'));
  const index = tabs.findIndex(tab => tab === event.target);
  if (index < 0) return;
  let next: number;
  switch (event.key) {
    case "Home": next = 0; break;
    case "End": next = tabs.length - 1; break;
    case "ArrowRight": next = (index + 1) % tabs.length; break;
    case "ArrowLeft": next = (index + tabs.length - 1) % tabs.length; break;
    default: return;
  }
  event.preventDefault();
  const target = tabs[next];
  if (target) {
    target.focus();
    target.click();
  }
}
