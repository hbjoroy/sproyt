import { randomUUID } from "node:crypto";
import { readFile } from "node:fs/promises";
import { expect, test, type APIRequestContext } from "@playwright/test";
import { eventImage } from "./support/event-image";

async function command(request: APIRequestContext, participant: string, type: string, payload?: unknown) {
  const response = await request.post(`/api/v1/commands?participant=${encodeURIComponent(participant)}`, {
    data: { protocol: "sproyt.chat.v1", request_id: randomUUID(), type, ...(payload === undefined ? {} : { payload }) }
  });
  expect(response.ok()).toBeTruthy();
  const result = await response.json();
  expect(result.type, JSON.stringify(result)).not.toBe("error");
  return result;
}

test("real event backend and React UI retain rich messages, RSVP, aliases, media and current membership authority", async ({ request, page }) => {
  const suffix = randomUUID(); const owner = `events-owner-${suffix}`; const member = `events-member-${suffix}`; const outsider = `events-outside-${suffix}`;
  for (const participant of [owner, member, outsider]) expect((await request.get(`/api/v1/me/emojis?participant=${participant}`)).ok()).toBeTruthy();
  const self = (await command(request, owner, "hello")).payload.participant_id;
  const memberId = (await command(request, member, "hello")).payload.participant_id;
  const outsiderId = (await command(request, outsider, "hello")).payload.participant_id;
  const circle = (await command(request, owner, "create_circle", { slug: `circle-${suffix}`, name: "Ekte arrangementskrets" })).payload.circle.id;
  const invitation = (await command(request, owner, "create_circle_invitation", { circle_id: circle })).payload.invitation;
  await command(request, member, "accept_circle_invitation", { token: invitation.token });
  const created = await command(request, owner, "create_channel", { slug: `events-${suffix}`, name: "Ekte arrangementsprøve", kind: "public", circle_id: circle });
  const channel = created.payload.channel.id;
  await command(request, member, "join_channel", { channel: { type: "id", value: channel } });
  const root = `/api/v1/channels/${channel}`;
  const path = `${root}/events`;
  const actor = (participant: string, user: string) => ({ query: `?participant=${encodeURIComponent(participant)}`, headers: { "x-sproyt-account": user } });
  const a = actor(owner, self); const b = actor(member, memberId); const c = actor(outsider, outsiderId);
  const input = { title: "Konsert frå ekte backend", code: "konsert", starts_at: "2027-06-01T17:00:00Z", timezone: "Europe/Oslo", ends_at: null, location: "Grendahuset", description: "**Ta med vener**" };
  const createdEvent = await request.post(`${path}${b.query}`, { headers: b.headers, data: input });
  expect(createdEvent.ok(), await createdEvent.text()).toBeTruthy(); const event = await createdEvent.json();
  expect(event.can_manage).toBe(true);
  expect(await (await request.get(`${root}/topics${a.query}`, { headers: a.headers })).json()).toMatchObject({ enabled: true });
  expect((await request.put(`${root}/topics${a.query}`, { headers: a.headers, data: { enabled: false } })).ok()).toBeTruthy();
  const roster = await request.get(`${path}/${event.id}/responses${a.query}`, { headers: a.headers });
  expect(await roster.json()).toMatchObject({ counts: { attending: 0, declined: 0, following: 0, unanswered: 2 },
    responses: expect.arrayContaining([expect.objectContaining({ user_id: self, status: "unanswered" }), expect.objectContaining({ user_id: memberId, status: "unanswered" })]) });
  expect((await request.get(`${path}/${event.id}${c.query}`, { headers: c.headers })).status()).toBe(403);
  expect((await request.put(`${path}/${event.id}/rsvp${a.query}`, { headers: a.headers, data: { status: "following" } })).ok()).toBeTruthy();
  const upload = await request.post(`${root}/media${a.query}`, { multipart: { file: {
    name: "konsert.png", mimeType: "image/png", buffer: await eventImage(page)
  } } });
  expect(upload.ok(), await upload.text()).toBeTruthy(); const media = await upload.json();
  const mediaId = media.id ?? media.media?.id;
  expect(typeof mediaId).toBe("string");
  expect((await request.post(`${path}/${event.id}/images${a.query}`, { headers: a.headers, data: { media_id: mediaId } })).ok()).toBeTruthy();
  expect((await request.put(`${path}/${event.id}/cover${b.query}`, { headers: b.headers, data: { media_id: mediaId } })).ok()).toBeTruthy();
  const original = await command(request, owner, "send_message", { channel_id: channel, body: `**Rik melding** #arr:konsert\n\n[[media:${mediaId}|image/png|konsert.png]]` });
  const message = original.payload.message;
  expect(message.id).toBeTruthy();
  const reply = await command(request, member, "send_message", { channel_id: channel, parent_message_id: message.id, body: "Svar i tråden #arr:konsert" });
  await command(request, member, "toggle_message_reaction", { message_id: message.id, emoji: "👍" });
  await command(request, member, "send_message", { channel_id: channel, body: `#arr:konsert [Kontekst](/?channel=${channel}&message=${message.id}&sequence=${message.sequence})` });
  const matches = await request.get(`${path}/${event.id}/messages${a.query}`, { headers: a.headers });
  expect((await matches.json()).messages.map((item: { id: string }) => item.id)).toEqual(expect.arrayContaining([message.id, reply.payload.message.id]));
  const edited = await request.put(`${path}/${event.id}${b.query}`, { headers: b.headers, data: { ...input, code: "ny-konsert", location: "Ny sal" } });
  expect(edited.ok()).toBeTruthy();
  const attention = await request.get(`${path}/${event.id}${a.query}`, { headers: a.headers });
  expect(await attention.json()).toMatchObject({ needs_attention: true, my_rsvp: "following", aliases: expect.arrayContaining(["konsert"]) });
  expect(await (await request.get(`${path}${a.query}`, { headers: a.headers })).json()).toMatchObject({ attention_count: 1 });
  const references = await request.post(`${path}/references${a.query}`, { headers: a.headers, data: { codes: ["konsert"] } });
  expect((await references.json()).events[0].id).toBe(event.id);
  for (const prefix of ["kon", "ny-"]) {
    const suggestions = await request.get(`${path}${a.query}&prefix=${prefix}`, { headers: a.headers });
    expect((await suggestions.json()).events.map((item: { id: string }) => item.id)).toContain(event.id);
  }
  // Force rich hits out of the ordinary fifty-message subscription window.
  for (let index = 0; index < 60; index++) await command(request, owner, "send_message", { channel_id: channel, body: `Nyare melding ${index}` });
  const bundle = await readFile(new URL("../dist/app.js", import.meta.url), "utf8");
  await page.route(/\/assets\/app\/[^/]+\/app.js$/, route => route.fulfill({ contentType: "text/javascript", body: bundle }));
  await page.goto(`/?participant=${owner}&channel=${channel}&ui=react&event=${event.id}`);
  const app = page.locator("#sproyt-react-preview");
  await expect(app.getByRole("heading", { level: 2, name: input.title })).toBeVisible();
  await expect(app.getByText(/^Arrangementet er endra sidan du svarte\./u)).toBeVisible();
  await app.getByRole("button", { name: "Markér som lesen" }).click();
  await expect(app.getByRole("button", { name: "Markér som lesen" })).toHaveCount(0);
  await app.getByRole("tab", { name: "Meldingar", exact: true }).click();
  const pills = app.getByRole("group", { name: "Arrangement i meldingane" });
  await pills.getByRole("button", { name: "#arr:ny-konsert", exact: true }).click();
  const results = app.getByLabel("Meldingar om arrangementet");
  await expect(results.getByText("Rik melding", { exact: true })).toBeVisible();
  await expect(results.getByRole("img", { name: "konsert.png", exact: true })).toBeVisible();
  await expect(results.getByText("Svar i tråden", { exact: false })).toBeVisible();
  await expect(results.getByRole("button", { name: "👍: 1 reaksjonar", exact: true })).toBeVisible();
  await expect(results.getByRole("link", { name: "Kontekst", exact: true })).toHaveAttribute("href", new RegExp(message.id));
  await command(request, owner, "edit_message", { message_id: message.id, body: "Meldinga er ikkje lenger merkt" });
  await expect(results.getByText("Meldinga er ikkje lenger merkt", { exact: true })).toHaveCount(0);
  await expect(results.getByText("Rik melding", { exact: true })).toHaveCount(0);
  // Owner may delegate governance to an existing member, preserving membership and RSVP while hidden.
  expect((await request.put(`${root}/members/${memberId}/role${a.query}`, { headers: a.headers, data: { role: "moderator" } })).ok()).toBeTruthy();
  expect((await request.put(`${root}/hidden${b.query}`, { headers: b.headers, data: { hidden: true } })).ok()).toBeTruthy();
  expect((await request.get(`${path}/${event.id}${a.query}`, { headers: a.headers })).ok()).toBeTruthy();
  await command(request, member, "leave_channel", { channel_id: channel });
  expect((await request.get(`${path}/${event.id}${b.query}`, { headers: b.headers })).status()).toBe(403);
  await command(request, member, "join_channel", { channel: { type: "id", value: channel } });
  expect(await (await request.get(`${path}/${event.id}${b.query}`, { headers: b.headers })).json()).toMatchObject({ my_rsvp: "unanswered" });
  // Exercise the create, edit, RSVP, image and cancellation commands through the real UI as well.
  await app.getByRole("tab", { name: /Arrangement/ }).click();
  await app.getByRole("button", { name: "Alle arrangement", exact: false }).click();
  await app.getByRole("button", { name: "Nytt arrangement", exact: true }).click();
  await app.getByRole("textbox", { name: "Tittel", exact: true }).fill("Oppretta i grensesnittet");
  await app.getByRole("textbox", { name: "Kode i kanalen", exact: true }).fill("friluft");
  await app.getByRole("textbox", { name: "Tidssone", exact: true }).fill("Europe/Oslo");
  await app.getByLabel("Start", { exact: true }).fill("2027-06-02T19:00");
  await app.getByRole("textbox", { name: "Omtale (Markdown)", exact: true }).fill("**Tur frå ekte grensesnitt**");
  await app.getByRole("button", { name: "Lagre arrangement" }).click();
  await expect(app.getByRole("heading", { level: 2, name: "Oppretta i grensesnittet" })).toBeVisible();
  expect(await (await request.get(`${root}/topics${a.query}`, { headers: a.headers })).json()).toMatchObject({ enabled: false });
  await app.getByRole("radio", { name: "Deltek", exact: true }).click();
  await expect(app.getByRole("radio", { name: "Deltek", exact: true })).toBeChecked();
  await app.getByLabel("Legg til bilete", { exact: true }).setInputFiles({ name: "tur.png", mimeType: "image/png", buffer: await eventImage(page) });
  await app.getByRole("button", { name: "Bruk som forside", exact: true }).click();
  await expect(app.getByRole("button", { name: "Fjern forside", exact: true })).toBeVisible();
  await app.getByRole("button", { name: "Rediger", exact: true }).click();
  await app.getByRole("textbox", { name: "Stad (valfritt)", exact: true }).fill("Uteplassen");
  await app.getByRole("button", { name: "Lagre arrangement" }).click();
  await expect(app.getByText("Uteplassen", { exact: true })).toBeVisible();
  await app.getByRole("button", { name: "Avlys arrangement", exact: true }).click();
  await app.getByRole("button", { name: "Stadfest avlysing", exact: true }).click();
  await expect(app.getByText("Arrangementet er avlyst.", { exact: true })).toBeVisible();
});
