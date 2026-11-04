using System.Collections.Concurrent;
using System.Net;
using System.Net.WebSockets;
using System.Text.Json;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Hosting;
using Microsoft.AspNetCore.Hosting.Server;
using Microsoft.AspNetCore.Hosting.Server.Features;
using Microsoft.AspNetCore.Http;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;

internal sealed record WireMessage(string id, string channel_id, string sender_id, string sender_display_name, string body, ulong sequence,
    string sent_at = "2026-11-04T12:00:00Z", string? parent_message_id = null, string? edited_at = null, string? deleted_at = null);
internal sealed class ConversationServer : IAsyncDisposable
{
    internal const string A = FixtureServer.Channel, B = "d0fa6389-d652-4058-9535-e06a05d2d97d", User = FixtureServer.User;
    internal const string Other = "aabbccdd-0000-4000-8000-000000000001";
    internal static string Id(ulong value) => $"{value:x8}-0000-4000-8000-000000000001";
    private readonly WebApplication app;
    private readonly SemaphoreSlim writes = new(1,1);
    private readonly ConcurrentBag<Task> tasks = [];
    internal ConcurrentDictionary<string,WireMessage> Messages { get; } = new();
    internal ConcurrentDictionary<string,int> Calls { get; } = new();
    internal HashSet<string> ReactionUsers { get; } = [];
    private WebSocket? current;
    internal Uri SocketUri { get; }
    internal string? HoldType, DenyType, WrongAckType, DropType;
    internal bool OversizeNextThread;
    internal TaskCompletionSource Held { get; private set; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
    internal TaskCompletionSource Release { get; private set; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
    internal ulong ReadSequence;
    internal void Hold(string type) { HoldType=type; Held=new(TaskCreationOptions.RunContinuationsAsynchronously); Release=new(TaskCreationOptions.RunContinuationsAsynchronously); }
    private ConversationServer(WebApplication app, Uri uri)
    {
        this.app=app; SocketUri=uri;
        foreach (var message in new[] { Message(1), Message(2), Message(3,1), Message(4,1), Message(5,2) }) Messages[message.id]=message;
    }
    internal static WireMessage Message(ulong seq, ulong? root = null) => new(Id(seq), A, User, "Synthetic owner", $"Message {seq}", seq, parent_message_id: root is ulong value ? Id(value) : null);
    internal static async Task<ConversationServer> StartAsync()
    {
        var builder=WebApplication.CreateSlimBuilder(); builder.Logging.ClearProviders();
        builder.WebHost.ConfigureKestrel(o=>o.Listen(IPAddress.Loopback,0));
        var app=builder.Build(); app.UseWebSockets(); ConversationServer? fixture=null;
        app.Map("/conversation",async context=>
        {
            using var socket=await context.WebSockets.AcceptWebSocketAsync(); fixture!.current=socket;
            try { await fixture.Serve(socket,context.RequestAborted); }
            catch (Exception failure) when (failure is WebSocketException or OperationCanceledException or IOException) { }
        });
        await app.StartAsync(); string address=app.Services.GetRequiredService<IServer>().Features.Get<IServerAddressesFeature>()!.Addresses.Single();
        return fixture=new(app,new UriBuilder(address){Scheme="ws",Path="/conversation"}.Uri);
    }
    private async Task Send(WebSocket socket,string type,object payload,string? requestId=null)
    {
        byte[] bytes=JsonSerializer.SerializeToUtf8Bytes(new { protocol="sproyt.chat.v1",request_id=requestId,type,payload });
        await writes.WaitAsync(); try { if(socket.State==WebSocketState.Open) await socket.SendAsync(bytes,WebSocketMessageType.Text,true,CancellationToken.None); } finally {writes.Release();}
    }
    internal Task MessageEvent(WireMessage message,string type="message_accepted")
    {
        Messages[message.id]=message;
        return Send(current!,"chat",new { @event=new { type,message }});
    }
    internal Task ReactionEvent(string user,bool added)
    {
        uint count; lock(ReactionUsers) {if(added) ReactionUsers.Add(user);else ReactionUsers.Remove(user);count=(uint)ReactionUsers.Count;}
        return Send(current!,"chat",new { @event=new {type="message_reaction_changed",change=new {message_id=Id(1),channel_id=A,user_id=user,emoji="👍",added,count}}});
    }
    private object Summary(string root)
    {
        var replies=Messages.Values.Where(m=>m.parent_message_id==root).ToArray();
        return new {root_message_id=root,reply_count=(uint)replies.Count(m=>m.deleted_at is null),unread_count=(uint)replies.Count(m=>m.deleted_at is null&&m.sequence>ReadSequence&&m.sender_id!=User),latest_sequence=replies.Select(m=>m.sequence).DefaultIfEmpty().Max()};
    }
    private async Task Serve(WebSocket socket,CancellationToken ct)
    {
        while(socket.State==WebSocketState.Open)
        {
            using var input=new MemoryStream();var buffer=new byte[4096];WebSocketReceiveResult part;
            do {part=await socket.ReceiveAsync(buffer,ct);if(part.MessageType==WebSocketMessageType.Close)return;input.Write(buffer,0,part.Count);}while(!part.EndOfMessage);
            using var json=JsonDocument.Parse(input.ToArray());var request=json.RootElement.Clone();
            tasks.Add(Respond(socket,request,ct));
        }
    }
    private async Task Respond(WebSocket socket,JsonElement request,CancellationToken ct)
    {
        string id=request.GetProperty("request_id").GetString()!,type=request.GetProperty("type").GetString()!;
        Calls.AddOrUpdate(type,1,(_,n)=>n+1);
        var p=request.GetProperty("payload");string response;object payload;
        switch(type)
        {
            case "hello": response="hello";payload=new{participant_id=User};break;
            case "list_my_circles":response="circles_listed";payload=new{circles=Array.Empty<object>()};break;
            case "list_my_channels":response="channels_listed";payload=new{channels=new[]{A,B}.Select(channel=>new{id=channel,name=channel,kind="private",last_read_sequence=0UL,latest_sequence=5UL})};break;
            case "subscribe_channel":response="subscription_started";payload=new{channel_id=p.GetProperty("channel_id").GetString(),history=Array.Empty<object>()};break;
            case "unsubscribe_channel":response="subscription_ended";payload=new{channel_id=p.GetProperty("channel_id").GetString()};break;
            case "load_recent_messages": response="messages_loaded";payload=new{channel_id=p.GetProperty("channel_id").GetString(),messages=Messages.Values.Where(m=>m.channel_id==p.GetProperty("channel_id").GetString()).OrderBy(m=>m.sequence).Take(50).ToArray()};break;
            case "load_thread":
                string root=p.GetProperty("root_message_id").GetString()!;response="thread_loaded";payload=new{root_message_id=root,messages=Messages.Values.Where(m=>m.id==root||m.parent_message_id==root).OrderBy(m=>m.sequence).ToArray()};
                if(OversizeNextThread)
                {
                    OversizeNextThread=false;
                    var header=System.Text.Encoding.UTF8.GetBytes("{\"protocol\":\"sproyt.chat.v1\",\"request_id\":\""+id+"\",\"type\":\"thread_loaded\",\"payload\":{\"root_message_id\":\""+root+"\",\"messages\":[");
                    string largeBody=new('x',64*1024);
                    await writes.WaitAsync(ct);try
                    {
                        await socket.SendAsync(header,WebSocketMessageType.Text,false,ct);
                        await socket.SendAsync(JsonSerializer.SerializeToUtf8Bytes(Messages[root]),WebSocketMessageType.Text,false,ct);
                        for(ulong n=10;n<331;n++)
                        {
                            await socket.SendAsync(new byte[]{(byte)','},WebSocketMessageType.Text,false,ct);
                            await socket.SendAsync(JsonSerializer.SerializeToUtf8Bytes(Message(n,1) with{body=largeBody}),WebSocketMessageType.Text,false,ct);
                        }
                        await socket.SendAsync(System.Text.Encoding.UTF8.GetBytes("]}}"),WebSocketMessageType.Text,true,ct);
                    }finally{writes.Release();}
                    return;
                }
                break;
            case "list_thread_summaries":response="thread_summaries_listed";payload=new{channel_id=p.GetProperty("channel_id").GetString(),summaries=Messages.Values.Where(m=>m.parent_message_id is not null).Select(m=>m.parent_message_id!).Distinct().Select(Summary).ToArray()};break;
            case "mark_thread_read":ReadSequence=p.GetProperty("sequence").GetUInt64();response="thread_read_updated";payload=new{summary=Summary(p.GetProperty("root_message_id").GetString()!)};break;
            case "list_channel_reactions":
                response="channel_reactions_listed";string[] users;lock(ReactionUsers)users=ReactionUsers.ToArray();
                payload=new{channel_id=p.GetProperty("channel_id").GetString(),reactions=users.Length==0?Array.Empty<object>():[new{message_id=Id(1),emoji="👍",count=(uint)users.Length,reacted_by_me=users.Contains(User),user_ids=users}]};break;
            case "toggle_message_reaction":
                response="message_reaction_changed";bool added;uint count;lock(ReactionUsers){added=!ReactionUsers.Remove(User);if(added)ReactionUsers.Add(User);count=(uint)ReactionUsers.Count;}
                payload=new{change=new{message_id=WrongAckType==type?Id(2):p.GetProperty("message_id").GetString(),channel_id=A,user_id=User,emoji=p.GetProperty("emoji").GetString(),added,count}};break;
            case "edit_message":case "delete_message":
                string messageId=p.GetProperty("message_id").GetString()!;var prior=Messages[messageId];
                var updated=type=="edit_message"?prior with{body=p.GetProperty("body").GetString()!.Trim(),edited_at="2026-11-04T14:00:00Z"}:prior with{body="",deleted_at="2026-11-04T15:00:00Z"};
                Messages[messageId]=updated;response=type=="edit_message"?"message_edited":"message_deleted";
                payload=new{message=WrongAckType==type?updated with{id=Id(99)}:updated};break;
            case "send_message":response="message_accepted";payload=new{message=Message(100) with{body=WrongAckType==type?"Wrong server body":p.GetProperty("body").GetString()!.Trim(),parent_message_id=p.GetProperty("parent_message_id").ValueKind==JsonValueKind.Null?null:p.GetProperty("parent_message_id").GetString()}};break;
            case "ping":response="pong";payload=new{};break;
            default:throw new InvalidOperationException("Unexpected conversation command: "+type);
        }
        if(HoldType==type){HoldType=null;Held.TrySetResult();await Release.Task.WaitAsync(ct);}
        if(DenyType==type){DenyType=null;response="error";payload=new{code="permission_denied",message="Synthetic access lost"};}
        if(DropType==type){DropType=null;socket.Abort();return;}
        await Send(socket,response,payload,id);
    }
    public async ValueTask DisposeAsync(){Release.TrySetResult();current?.Abort();await app.StopAsync();await app.DisposeAsync();foreach(var task in tasks){try{await task;}catch{}}}
}
