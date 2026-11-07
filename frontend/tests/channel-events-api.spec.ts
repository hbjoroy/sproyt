import { randomUUID } from "node:crypto";
import { readFile } from "node:fs/promises";
import { expect, test, type APIRequestContext } from "@playwright/test";

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
  const self = (await command(request, owner, "hello")).payload.participant_id;
  const memberId = (await command(request, member, "hello")).payload.participant_id;
  const outsiderId = (await command(request, outsider, "hello")).payload.participant_id;
  const created = await command(request, owner, "create_channel", { slug: `events-${suffix}`, name: "Ekte arrangementsprøve", kind: "public", circle_id: null });
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
  expect((await request.get(`${path}/${event.id}${c.query}`, { headers: c.headers })).status()).toBe(403);
  expect((await request.put(`${path}/${event.id}/rsvp${a.query}`, { headers: a.headers, data: { status: "following" } })).ok()).toBeTruthy();
  const upload = await request.post(`${root}/media${a.query}`, { multipart: { file: {
    name: "konsert.png", mimeType: "image/png", buffer: Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a6aQAAAAASUVORK5CYII=", "base64")
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
  const matches = await request.get(`${path}/${event.id}/messages${a.query}`, { headers: a.headers });
  expect((await matches.json()).messages.map((item: { id: string }) => item.id)).toEqual(expect.arrayContaining([message.id, reply.payload.message.id]));
  const edited = await request.put(`${path}/${event.id}${b.query}`, { headers: b.headers, data: { ...input, code: "ny-konsert", location: "Ny sal" } });
  expect(edited.ok()).toBeTruthy();
  const attention = await request.get(`${path}/${event.id}${a.query}`, { headers: a.headers });
  expect(await attention.json()).toMatchObject({ needs_attention: true, my_rsvp: "following", aliases: expect.arrayContaining(["konsert"]) });
  const references = await request.post(`${path}/references${a.query}`, { headers: a.headers, data: { codes: ["konsert"] } });
  expect((await references.json()).events[0].id).toBe(event.id);
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
});
