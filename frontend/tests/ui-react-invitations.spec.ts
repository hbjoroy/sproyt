import { expect, test, type Page } from "@playwright/test";

const token = "react_invitation_0123456789abcdef0123456789";

async function mockInvitations(page: Page, mode: "flow" | "errors" | "owner" = "flow") {
  await page.addInitScript(({ mode, token }) => {
    const NativeWebSocket = window.WebSocket;
    const commands: string[] = [];
    Object.assign(window, { invitationTestCommands: commands });
    let participantId = "";
    let response: "accepted" | "declined" | null = null;
    let inspectionAttempts = 0;
    let acceptAttempts = 0;
    let channelId = "";
    window.WebSocket = class extends NativeWebSocket {
      constructor(url: string | URL, protocols?: string | string[]) {
        super(url, protocols);
        this.addEventListener("message", event => {
          const frame = JSON.parse(event.data);
          if (frame.type === "hello") participantId = frame.payload.participant_id;
        });
      }
      send(data: string): void {
        const command = JSON.parse(data);
        if (command.type === "subscribe_channel") channelId = command.payload.channel_id;
        if (!["inspect_invitation", "accept_invitation", "decline_invitation"].includes(command.type)) {
          super.send(data); return;
        }
        commands.push(command.type);
        const emit = (type: string, payload: unknown) => setTimeout(() => this.dispatchEvent(new MessageEvent("message", {
          data: JSON.stringify({ protocol: "sproyt.chat.v1", type, request_id: command.request_id, payload })
        })), 120);
        if (command.payload.token !== token) {
          emit("error", { code: "not_found", message: "No such invitation" }); return;
        }
        if (command.type === "inspect_invitation" && mode === "errors" && inspectionAttempts++ === 0) {
          emit("error", { code: "internal_error", message: "Temporary failure" }); return;
        }
        if (command.type === "accept_invitation") {
          if (mode === "flow" && acceptAttempts++ === 0) {
            emit("error", { code: "permission_denied", message: "Permission denied" }); return;
          }
          response = "accepted";
          emit("invitation_accepted", { token, invitation: {
            target: { type: "circle", circle_id: "invitation-circle" },
            channel: { id: channelId, circle_id: null, slug: "general", name: "General", kind: "public", created_by: participantId }
          } }); return;
        }
        if (command.type === "decline_invitation") response = "declined";
        emit(command.type === "decline_invitation" ? "invitation_declined" : "invitation_inspected", {
          token, invitation: {
            target: { type: "circle", circle_id: "invitation-circle" }, circle_name: "React-venene",
            channel_name: null, invited_by: mode === "owner" ? participantId : "someone-else",
            invited_by_name: "Ada <img src=x onerror=alert(1)>", expires_at: "2027-01-01T00:00:00Z",
            response, accepted_count: mode === "owner" ? 2 : 0, declined_count: mode === "owner" ? 1 : 0
          }
        });
      }
    };
  }, { mode, token });
}

async function sendInvitation(page: Page, mode: "flow" | "errors" | "owner") {
  await mockInvitations(page, mode);
  await page.goto(`/?participant=react-invitation-${mode}-${Date.now()}&ui=react`);
  const preview = page.locator("#sproyt-react-preview");
  const composer = preview.getByRole("textbox", { name: "Skriv melding" });
  await expect(composer).toBeEnabled({ timeout: 15_000 });
  const marker = `invitasjonskort-${mode}-${Date.now()}`;
  await composer.fill(`${marker}\n[[invite:${token}]]`);
  await composer.press("Enter");
  const message = preview.locator(".sp-channel-pane [data-message-id]").filter({ hasText: marker });
  const card = message.getByRole("region", { name: "Invitasjon", exact: true });
  await expect(card).toBeVisible();
  return { preview, message, card, marker };
}

test("React invitation shares actions and failure recovery between channel and thread, and keeps acceptance after remount", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", error => errors.push(error.message));
  const { preview, message, card } = await sendInvitation(page, "flow");
  await expect(card).toContainText("Ada <img src=x onerror=alert(1)> har invitert deg.");
  await expect(card.locator("img")).toHaveCount(0);
  await message.getByRole("button", { name: "Svar i tråd" }).click();
  const thread = preview.locator(".sp-thread-pane");
  const threadCard = thread.getByRole("region", { name: "Invitasjon", exact: true }).first();
  await expect(threadCard).toContainText("Invitasjon til vennekretsen React-venene");
  const reply = thread.getByRole("textbox", { name: "Svar i tråden" });
  await reply.fill(`Invitasjon i svaret [[invite:${token}]]`);
  await reply.press("Enter");
  const replyCard = thread.locator("[data-message-id]").filter({ hasText: "Invitasjon i svaret" }).getByRole("region", { name: "Invitasjon", exact: true });
  await expect(replyCard.getByRole("button", { name: "Godta", exact: true })).toBeEnabled();
  await threadCard.getByRole("button", { name: "Godta", exact: true }).click();
  await expect(card).toHaveAttribute("aria-busy", "true");
  await expect(card.getByRole("button", { name: "Godta", exact: true })).toBeDisabled();
  await expect(threadCard.getByRole("alert")).toContainText("Du må først vere medlem i vennekretsen");
  await expect(card.getByRole("alert")).toContainText("Du må først vere medlem i vennekretsen");
  await expect(replyCard.getByRole("alert")).toContainText("Du må først vere medlem i vennekretsen");
  await threadCard.getByRole("button", { name: "Avvis", exact: true }).click();
  await expect(card).toContainText("Du har avvist invitasjonen.");
  await expect(threadCard.getByRole("button", { name: "Avvis", exact: true })).toBeDisabled();
  await threadCard.getByRole("button", { name: "Godta likevel", exact: true }).click();
  await expect(card).toContainText("Du har godteke invitasjonen.");
  await page.keyboard.press("Escape");
  await message.getByRole("button", { name: "Svar i tråd" }).click();
  await expect(threadCard).toContainText("Du har godteke invitasjonen.");
  await expect(replyCard).toContainText("Du har godteke invitasjonen.");
  await expect(threadCard.getByRole("button")).toHaveCount(0);
  expect(errors).toEqual([]);
});

test("React invitation retries transient inspection errors and leaves invalid tokens as text", async ({ page }) => {
  const { preview, card } = await sendInvitation(page, "errors");
  await expect(card.getByRole("alert")).toContainText("Invitasjonen kunne ikkje hentast no.");
  await card.getByRole("button", { name: "Prøv igjen" }).click();
  await expect(card.getByRole("button", { name: "Godta", exact: true })).toBeEnabled();
  const composer = preview.getByRole("textbox", { name: "Skriv melding" });
  const invalid = "[[invite:too-short]] [[invite:" + "a".repeat(129) + "]] [[invite:bad<script>token]]";
  await composer.fill(invalid);
  await composer.press("Enter");
  const invalidMessage = preview.locator(".sp-channel-pane [data-message-id]").filter({ hasText: "too-short" });
  await expect(invalidMessage).toContainText(invalid);
  await expect(invalidMessage.locator(".invitation-card, script")).toHaveCount(0);
  await composer.fill(`missing-token [[invite:${"missing".repeat(6)}]]`);
  await composer.press("Enter");
  const missing = preview.locator(".sp-channel-pane [data-message-id]").filter({ hasText: "missing-token" });
  await expect(missing.getByRole("alert")).toHaveText("Invitasjonen finst ikkje eller er ikkje gyldig lenger.");
  await expect(missing.getByRole("button", { name: "Prøv igjen" })).toHaveCount(0);
});

test("React invitation author sees response counts and cannot accept their own invitation", async ({ page }) => {
  const { card } = await sendInvitation(page, "owner");
  await expect(card).toContainText("Du sende invitasjonen. 2 har godteke, 1 har avvist.");
  await expect(card.getByRole("button")).toHaveCount(0);
});

test("React invitation completes a real server decline and acceptance, and refreshes the author's response counts", async ({ page, context }) => {
  await page.goto(`/?participant=react-invitation-real-owner-${Date.now()}&ui=react`);
  const owner = page.locator("#sproyt-react-preview");
  await expect(owner.getByRole("textbox", { name: "Skriv melding" })).toBeEnabled({ timeout: 15_000 });
  if (!await owner.getByRole("button", { name: "Meny og innstillingar", exact: true }).isVisible()) {
    await owner.getByRole("button", { name: "Meny", exact: true }).click();
  }
  await owner.getByRole("button", { name: "Meny og innstillingar", exact: true }).click();
  await owner.getByRole("dialog", { name: "Meny og innstillingar" }).getByRole("button", { name: "Ny vennekrets", exact: true }).click();
  const circleName = `Invitasjonstest ${Date.now()}`;
  const create = owner.getByRole("dialog", { name: "Ny vennekrets", exact: true });
  await create.getByLabel("Namn på vennekrets").fill(circleName);
  await create.getByRole("button", { name: "Opprett vennekrets" }).click();
  await expect(create).toHaveCount(0);
  await owner.getByRole("region", { name: circleName, exact: true }).getByRole("button", { name: "Inviter personar og nye brukarar" }).click();
  const invite = owner.getByRole("dialog", { name: `Inviter til ${circleName}` });
  await invite.getByRole("button", { name: "Lag kretslenkje" }).click();
  const link = invite.getByRole("textbox", { name: "Invitasjonslenkje", exact: true });
  await expect(link).toHaveValue(/invite=/);
  const realToken = new URL(await link.inputValue()).searchParams.get("invite")!;
  await page.keyboard.press("Escape");
  await expect(owner.getByRole("dialog", { name: "Meny og innstillingar" })).toBeVisible();
  await page.keyboard.press("Escape");
  await owner.getByRole("button", { name: /^# general/i }).click();
  const composer = owner.getByRole("textbox", { name: "Skriv melding" });
  await composer.fill(`${circleName} [[invite:${realToken}]]`);
  await composer.press("Enter");
  const ownerCard = owner.locator(".sp-channel-pane [data-message-id]").filter({ hasText: circleName }).getByRole("region", { name: "Invitasjon", exact: true });
  await expect(ownerCard).toContainText("Du sende invitasjonen. Ventar på svar.");
  const peer = await context.newPage();
  await peer.goto(`/?participant=react-invitation-real-peer-${Date.now()}&ui=react`);
  const member = peer.locator("#sproyt-react-preview");
  const memberCard = member.locator(".sp-channel-pane [data-message-id]").filter({ hasText: circleName }).getByRole("region", { name: "Invitasjon", exact: true });
  await memberCard.getByRole("button", { name: "Avvis", exact: true }).click();
  await expect(memberCard).toContainText("Du har avvist invitasjonen.");
  await page.bringToFront();
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect(ownerCard).toContainText("1 har avvist");
  await peer.bringToFront();
  await memberCard.getByRole("button", { name: "Godta likevel", exact: true }).click();
  await expect(member.getByRole("heading", { name: circleName, exact: true })).toBeVisible();
  await member.getByRole("button", { name: /^# general/i }).click();
  await expect(memberCard).toContainText("Du har godteke invitasjonen.");
  await expect(memberCard.getByRole("button")).toHaveCount(0);
  await page.bringToFront();
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect(ownerCard).toContainText("1 har godteke");
  await peer.close();
});
