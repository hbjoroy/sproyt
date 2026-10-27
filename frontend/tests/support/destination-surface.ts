import type { Locator, Page } from "@playwright/test";

/** A routed destination is a named workspace region; standalone hosts use a dialog. */
export function destinationSurface(scope: Locator | Page, name: string | RegExp, exact = true): Locator {
  return scope.getByRole("region", { name, exact }).and(scope.locator("[data-workspace-page]"))
    .or(scope.getByRole("dialog", { name, exact }));
}
