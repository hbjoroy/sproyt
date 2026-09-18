import type { EnrollmentApi } from "../api";
import { isEnrollmentNotConfigured } from "../api";
import type { Channel, Circle, ClientCommand, ClientCommandArguments } from "../types";
import type { CommunityHost } from "../ui/react/preview-community";
import type { createCommunityRequests } from "./community-requests";

export function createCommunityHost(deps: {
  requests: ReturnType<typeof createCommunityRequests>;
  send: <T extends ClientCommand["type"]>(type: T, ...args: ClientCommandArguments<T>) => string | null;
  openDirect(userId: string): string | null;
  selfId(): string | null;
  channels(): readonly Channel[];
  circles(): ReadonlyMap<string, Circle>;
  slugify(value: string): string;
  channelSlug(circleId: string, name: string): string;
  invitationToken(value: string): string | null;
  enrollment: EnrollmentApi;
}): CommunityHost {
  const request = deps.requests.request;
  const circle = (id: string, owner = false) => {
    const value = deps.circles().get(id);
    if (!value || (owner && value.role !== "owner")) throw new Error("Du har ikkje tilgang til denne kretshandlinga.");
    return value;
  };
  const channel = (id: string, owner = false, manager = false) => {
    const value = deps.channels().find(item => item.id === id);
    if (!value || (owner && value.role !== "owner") || (manager && !["owner", "moderator"].includes(value.role))) throw new Error("Du har ikkje tilgang til denne kanalhandlinga.");
    return value;
  };
  return {
    selfId: deps.selfId,
    users: async () => (await request("users_listed", () => deps.send("list_users"))).payload.users,
    members: async id => (await request("channel_users_listed", () => deps.send("list_channel_users", { channel_id: id }))).payload.users,
    circleMembers: async id => (await request("circle_users_listed", () => deps.send("list_circle_users", { circle_id: id }))).payload.users,
    openDirect: async userId => { if (userId === deps.selfId()) throw new Error("Vel ein annan person."); await request("direct_channel_opened", () => deps.openDirect(userId)); },
    createCircle: async name => {
      if (name.trim().length < 2) throw new Error("Kretsnamnet må ha minst to teikn.");
      // The existing circle_created handler creates Prat exactly once.
      await request("circle_created", () => deps.send("create_circle", { name, slug: deps.slugify(name) }));
    },
    createChannel: async (id, name, kind) => {
      if (id) circle(id);
      const trimmed = name.trim();
      if (!trimmed) throw new Error("Skriv eit kanalnamn.");
      // Felles has no circle namespace. Keep its stable, human-readable slug
      // compatible with the established global channel contract.
      const slug = id ? deps.channelSlug(id, trimmed) : deps.slugify(trimmed).replace(/^-+|-+$/g, "");
      if (!slug) throw new Error("Kanalnamnet må innehalde bokstavar eller tal.");
      await request("channel_created", () => deps.send("create_channel", { circle_id: id, name: trimmed, kind, slug }));
    },
    joinable: async id => { circle(id); return (await request("joinable_channels_listed", () => deps.send("list_joinable_channels", { circle_id: id }))).payload.channels.map(item => ({ id: item.channel.id, name: item.channel.name, description: item.description })); },
    join: async id => { await request("membership_joined", () => deps.send("join_channel", { channel: { type: "id", value: id } })); },
    leaveChannel: async id => { channel(id); await request("membership_left", () => deps.send("leave_channel", { channel_id: id })); },
    leaveCircle: async id => { if (circle(id).role === "owner") throw new Error("Eigaren kan ikkje forlate kretsen."); await request("circle_left", () => deps.send("leave_circle", { circle_id: id })); },
    deleteCircle: async id => { circle(id, true); await request("circle_deleted", () => deps.send("delete_circle", { circle_id: id })); },
    description: async (id, description) => { channel(id, true); await request("channel_description_updated", () => deps.send("update_channel_description", { channel_id: id, description })); },
    addMember: async (id, userId) => { channel(id, false, true); await request("channel_member_added", () => deps.send("add_channel_member", { channel_id: id, user_id: userId })); },
    invite: async (circleId, channelId, userId) => {
      if (channelId) { if (channel(channelId, false, true).circle_id !== circleId) throw new Error("Kanalen høyrer ikkje til denne kretsen."); }
      else circle(circleId, true);
      const created = await request("invitation_created", () => deps.send("create_invitation", { target: channelId ? { type: "channel", circle_id: circleId, channel_id: channelId } : { type: "circle", circle_id: circleId } }));
      const token = created.payload.invitation.token;
      if (userId) {
        const direct = await request("direct_channel_opened", () => deps.openDirect(userId));
        await request("message_accepted", () => deps.send("send_message", { channel_id: direct.payload.channel.id, body: `[[invite:${token}]]` }));
      }
      return `${window.location.origin}/?invite=${encodeURIComponent(token)}`;
    },
    accept: async value => { const token = deps.invitationToken(value); if (!token) throw new Error("Skriv inn ein gyldig invitasjonskode eller lenkje."); await request("invitation_accepted", () => deps.send("accept_invitation", { token })); },
    enroll: async (circleId, email, name) => {
      circle(circleId, true);
      try { return await deps.enrollment.create(circleId, { email, displayName: name || undefined }); }
      catch (error) { if (isEnrollmentNotConfigured(error)) throw new Error("Registrering av nye brukarar er ikkje tilgjengeleg enno. Du kan framleis dele ei vanleg kretslenkje."); throw error; }
    },
    enrollGlobal: async (email, name) => {
      try { return await deps.enrollment.createGlobal({ email, displayName: name || undefined }); }
      catch (error) { if (isEnrollmentNotConfigured(error)) throw new Error("Registrering av nye brukarar er ikkje tilgjengeleg enno."); throw error; }
    }
  };
}
