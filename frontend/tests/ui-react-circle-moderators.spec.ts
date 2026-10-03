import { expect, test, type Page, type WebSocketRoute } from "@playwright/test";

const circleId = "00000000-0000-7000-8000-000000000001";
const channelId = "00000000-0000-7000-8000-000000000002";
type Role = "owner" | "moderator" | "member";
async function fixture(page: Page, initialRole: Role) {
  let role = initialRole;
  let peerRole: Role = "member";
  let socket: WebSocketRoute;
  let kind = "public";
  const commands: any[] = [];
  const circle = { id: circleId, slug: "moderator-circle", name: "Testkrets", created_by: "owner", created_at: "2026-01-01T00:00:00Z" };
  const channels = () => ({ channels: [{ id: channelId, slug: "moderator-prat", name: "Prat", kind, circle_id: circleId, direct_user_id: null, is_direct: false, description: "", role: "member", last_read_sequence: 1, latest_sequence: 1 }] });
  const profile = (id: string, name: string) => ({ id,kind:"human",display_name:name,handle:null,external_provider:null,external_subject:null,created_at:"2026-01-01T00:00:00Z",status_text:"",status_emoji:"",status_expires_at:null });
  const emit = (type: string, payload?: unknown, request_id?: string) => socket.send(JSON.stringify({ protocol: "sproyt.chat.v1", type, ...(payload === undefined ? {} : { payload }), ...(request_id ? { request_id } : {}) }));
  await page.route(/\/api\/v1\/circles\/[^/]+\/chat-agents$/, route=>route.fulfill({json:{agents:[],worker_available:false}}));
  await page.routeWebSocket(/\/ws(?:\?|$)/, route => {
    socket = route;
    route.onMessage(data => {
      const command = JSON.parse(String(data)); commands.push(command);
      const reply = (type: string, payload?: unknown) => emit(type, payload, command.request_id);
      switch (command.type) {
        case "hello": reply("hello", { participant_id: initialRole }); break;
        case "ping": reply("pong"); break;
        case "list_users": reply("users_listed", { users: [] }); break;
        case "list_my_circles": reply("circles_listed", { circles: [[circle, role]] }); break;
        case "list_circle_members": reply("circle_members_listed",{circle_id:circleId,members:[[profile("owner","Eigaren"),"owner"],[profile("peer","Kari"),peerRole]]});break;
        case "set_circle_member_role": peerRole=command.payload.role; reply("circle_member_role_changed",{membership:{circle_id:circleId,user_id:"peer",role:peerRole,joined_at:"2026-01-01T00:00:00Z"}});emit("circles_changed");break;
        case "list_mentions": reply("mentions_listed", { mentions: [] }); break;
        case "list_tasks": reply("tasks_listed", { tasks: [] }); break;
        case "list_my_channels": reply("channels_listed",channels()); break;
        case "subscribe_channel": reply("subscription_started",{channel_id:channelId,history:[{id:"peer-message",channel_id:channelId,sequence:1,sender_id:"peer",sender_display_name:"Kari",body:"Melding frå Kari",sent_at:"2026-01-01T00:00:00Z"}]}); break;
        case "list_thread_summaries": reply("thread_summaries_listed",{channel_id:channelId,summaries:[]});break;
        case "list_channel_reactions": reply("channel_reactions_listed",{channel_id:channelId,reactions:[]});break;
        case "load_recent_messages": reply("messages_loaded",{channel_id:channelId,messages:[]});break;
      }
    });
  });
  await page.goto(`/?participant=moderators-${initialRole}&channel=${channelId}`);
  const preview=page.locator("#sproyt-react-preview");
  await expect(preview.getByRole("textbox",{name:"Skriv melding",exact:true})).toBeEnabled();
  return {preview,commands,role:(value:Role)=>{role=value;emit("circles_changed");},private:()=>{kind="private";emit("membership_joined",{membership:{channel_id:channelId,user_id:initialRole,role:"member",joined_at:"2026-01-01T00:00:00Z",last_read_sequence:1}});}};
}

for (const width of [1280,390]) {
  test(`owner appoints and removes an existing human moderator from compact people rows at ${width}px`,async({page})=>{
    await page.setViewportSize({width,height:844});
    const server=await fixture(page,"owner");
    if(width===390) await server.preview.getByRole("button",{name:"Samtalar",exact:true}).click();
    await server.preview.getByRole("button",{name:"Val for Testkrets",exact:true}).click();
    await server.preview.getByRole("button",{name:"Medlemmer og roller i Testkrets",exact:true}).click();
    const dialog=server.preview.getByRole("dialog",{name:"Kretsmedlemmer og roller",exact:true});
    await expect(dialog).toContainText("Eigaren");
    await expect(dialog.getByRole("button",{name:/Eigaren/})).toHaveCount(0);
    await dialog.getByRole("button",{name:"Gjer Kari til moderator",exact:true}).click();
    await expect(dialog.getByRole("button",{name:"Fjern moderatorrolla frå Kari",exact:true})).toBeEnabled();
    await dialog.getByRole("button",{name:"Fjern moderatorrolla frå Kari",exact:true}).click();
    await expect(dialog.getByRole("button",{name:"Gjer Kari til moderator",exact:true})).toBeEnabled();
    expect(server.commands.filter(command=>command.type==="set_circle_member_role").map(command=>command.payload)).toEqual([{circle_id:circleId,user_id:"peer",role:"moderator"},{circle_id:circleId,user_id:"peer",role:"member"}]);
    expect(await dialog.evaluate(element=>element.scrollWidth<=element.clientWidth+1)).toBe(true);
  });

  test(`moderator has delete-only open-channel rights and loses agent UI on live demotion at ${width}px`,async({page})=>{
    await page.setViewportSize({width,height:844});
    const server=await fixture(page,"moderator");
    const message=server.preview.locator('[data-message-id="peer-message"]');
    await message.getByRole("button",{name:"Fleire meldingsval",exact:true}).click();
    await expect(message.getByRole("button",{name:"Slett",exact:true})).toHaveCount(1);
    await expect(message.getByRole("button",{name:"Rediger",exact:true})).toHaveCount(0);
    server.private();
    await expect(message.getByRole("button",{name:"Slett",exact:true})).toHaveCount(0);
    await page.keyboard.press("Escape");
    if(width===390) await server.preview.getByRole("button",{name:"Samtalar",exact:true}).click();
    await server.preview.getByRole("button",{name:"Val for Testkrets",exact:true}).click();
    await expect(server.preview.getByRole("button",{name:/Endre namn på|Inviter til/})).toHaveCount(0);
    await server.preview.getByRole("button",{name:"Medlemmer og roller i Testkrets",exact:true}).click();
    const people=server.preview.getByRole("dialog",{name:"Kretsmedlemmer og roller",exact:true});
    await expect(people).toContainText("Kari");
    await expect(people.getByRole("button",{name:/Gjer.*moderator|Fjern moderator/})).toHaveCount(0);
    await page.keyboard.press("Escape");
    await server.preview.getByRole("button",{name:"Val for Testkrets",exact:true}).click();
    await server.preview.getByRole("button",{name:"Agentar i Testkrets",exact:true}).click();
    await expect(server.preview.getByRole("dialog",{name:"Agentar i Testkrets",exact:true})).toBeVisible();
    server.role("member");
    await expect(server.preview.getByRole("dialog",{name:"Agentar i Testkrets",exact:true})).toHaveCount(0);
    await server.preview.getByRole("button",{name:"Val for Testkrets",exact:true}).click();
    await expect(server.preview.getByRole("button",{name:"Agentar i Testkrets",exact:true})).toHaveCount(0);
    await page.keyboard.press("Escape");
    await page.reload();
    await expect(server.preview.getByRole("textbox",{name:"Skriv melding",exact:true})).toBeEnabled();
    if(width===390) await server.preview.getByRole("button",{name:"Samtalar",exact:true}).click();
    await server.preview.getByRole("button",{name:"Val for Testkrets",exact:true}).click();
    await expect(server.preview.getByRole("button",{name:"Agentar i Testkrets",exact:true})).toHaveCount(0);
  });
}
