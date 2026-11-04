using System.Diagnostics;
using System.Text.Json;

const string stamp="synthetic:family:0";
void Check(bool value,[System.Runtime.CompilerServices.CallerLineNumber]int line=0){if(!value)throw new InvalidOperationException("Conversation assertion failed at "+line);}
async Task Until(Func<bool> condition){var time=Stopwatch.StartNew();while(!condition()){Check(time.Elapsed<TimeSpan.FromSeconds(10));await Task.Delay(10);}}
async Task Fails(Func<Task> run){try{await run();}catch{return;}throw new InvalidOperationException("Expected fenced/uncertain failure");}

await using(var server=await ConversationServer.StartAsync())
{
    var owner=new NativeAuthSession(server.SocketUri);await using var client=new NativeChatClient(owner);await client.StartAsync();
    long view=client.Snapshot.History!.ViewGeneration;string channel=ConversationServer.A,user=ConversationServer.User,root=ConversationServer.Id(1),reply=ConversationServer.Id(3);
    Check(client.Snapshot.Messages.Count==2&&client.Snapshot.Messages.All(m=>m.ParentMessageId is null));
    var summaries=await client.ListThreadSummariesAsync(channel,view,user,stamp);Check(summaries.Count==2&&summaries.Single(s=>s.RootMessageId==root).ReplyCount==2);
    var loaded=await client.LoadThreadAsync(channel,root,view,user,stamp);Check(loaded.Root?.Id==root&&loaded.Replies.Count==2&&loaded.LastReadSequence==0&&server.ReadSequence==0);
    await client.MarkThreadVisibleReadAsync(channel,root,view,loaded.Generation,[root,"unknown"],user,stamp);Check(server.ReadSequence==0);
    server.Hold("mark_thread_read");var reading=client.MarkThreadVisibleReadAsync(channel,root,view,loaded.Generation,[reply],user,stamp);await server.Held.Task.WaitAsync(TimeSpan.FromSeconds(8));
    Check(client.Snapshot.Conversation!.Thread!.LastReadSequence==0);server.Release.TrySetResult();await reading;Check(client.Snapshot.Conversation!.Thread!.LastReadSequence==3);
    // A held old root page cannot replace a newer A-B-A visit.
    server.Hold("load_thread");var old=client.LoadThreadAsync(channel,root,view,user,stamp);await server.Held.Task.WaitAsync(TimeSpan.FromSeconds(8));
    var current=await client.LoadThreadAsync(channel,ConversationServer.Id(2),view,user,stamp);server.Release.TrySetResult();await Fails(async()=>await old);
    Check(client.Snapshot.Conversation!.Thread!.RootMessageId==current.RootMessageId);
    // Stale page contents cannot revive a deleted reply or erase a live edit.
    server.Hold("load_thread");var stale=client.LoadThreadAsync(channel,root,view,user,stamp);await server.Held.Task.WaitAsync(TimeSpan.FromSeconds(8));
    await server.MessageEvent(ConversationServer.Message(3,1) with{body="",deleted_at="2026-11-04T16:00:00Z"},"message_deleted");
    await server.MessageEvent(ConversationServer.Message(4,1) with{body="Live edit",edited_at="2026-11-04T16:00:00Z"},"message_edited");
    await Until(()=>client.Snapshot.Conversation!.Thread!.Replies.Count==2);server.Release.TrySetResult();var merged=await stale;
    Check(merged.Replies.Single(m=>m.Id==reply).DeletedAt is not null&&merged.Replies.Single(m=>m.Sequence==4).Body=="Live edit");
    var accepted=await client.SendAsync(channel,"Reply", "reply-stable-id",user,stamp,parentMessageId:root);Check(accepted.ParentMessageId==root&&client.Snapshot.Messages.All(m=>m.Id!=accepted.Id));
    var trimmed=await client.SendAsync(channel,"  Trimmed reply \n", "reply-trimmed-id",user,stamp,parentMessageId:root);Check(trimmed.Body=="Trimmed reply");
    server.WrongAckType="send_message";await Fails(async()=>await client.SendAsync(channel,"Expected reply", "reply-wrong-body",user,stamp,parentMessageId:root));server.WrongAckType=null;
    Check(client.Snapshot.Conversation!.Thread!.Replies.All(m=>m.Body!="Wrong server body"));
    await client.EditMessageAsync(channel,root,"Edited root","edit1",view,user,stamp);Check(client.Snapshot.Messages.Single(m=>m.Id==root).Body=="Edited root");
    server.WrongAckType="edit_message";await Fails(async()=>await client.EditMessageAsync(channel,root,"Wrong ACK","edit-wrong",view,user,stamp));Check(client.Snapshot.Messages.All(m=>m.Id!=ConversationServer.Id(99))&&client.Snapshot.Messages.Single(m=>m.Id==root).Body=="Edited root");server.WrongAckType=null;
    var readback=await client.ReadMessageAsync(channel,root,root,view,user,stamp);Check(readback!.Body=="Wrong ACK"&&client.Snapshot.Conversation!.Thread!.RootMessageId==root);
    Check(!client.Snapshot.Conversation!.ReactionsLoaded);
    await client.ListChannelReactionsAsync(channel,view,user,stamp);
    Check(client.Snapshot.Conversation!.ReactionsLoaded);
    var change=await client.ToggleReactionAsync(channel,root,"👍","reaction1",view,user,stamp);Check(change.Added&&client.Snapshot.Conversation!.Reactions.Single().ReactedByMe);
    await server.ReactionEvent(user,true);await Until(()=>client.Snapshot.Conversation!.Reactions.Single().Count==1);Check(client.Snapshot.Conversation!.Reactions.Single().Count==1);
    server.Hold("list_channel_reactions");var snapshot=client.ListChannelReactionsAsync(channel,view,user,stamp);await server.Held.Task.WaitAsync(TimeSpan.FromSeconds(8));await server.ReactionEvent(ConversationServer.Other,true);
    await Until(()=>client.Snapshot.Conversation!.Reactions.Single().Count==2);server.Release.TrySetResult();Check((await snapshot).Single().Count==2);
    await client.DeleteMessageAsync(channel,root,"delete1",view,user,stamp);Check(client.Snapshot.Messages.Single(m=>m.Id==root).Body==""&&client.Snapshot.Conversation!.Reactions.All(r=>r.MessageId!=root));
    // Readback for another root does not open that thread.
    await client.ReadMessageAsync(channel,ConversationServer.Id(2),ConversationServer.Id(5),view,user,stamp);Check(client.Snapshot.Conversation!.Thread!.RootMessageId==root);
    // A denied read belonging to an earlier thread cannot clear the newer one.
    loaded=await client.LoadThreadAsync(channel,root,view,user,stamp);server.Hold("mark_thread_read");server.DenyType="mark_thread_read";
    var deniedOld=client.MarkThreadVisibleReadAsync(channel,root,view,loaded.Generation,[ConversationServer.Id(4)],user,stamp);await server.Held.Task.WaitAsync(TimeSpan.FromSeconds(8));
    var other=await client.LoadThreadAsync(channel,ConversationServer.Id(2),view,user,stamp);server.Release.TrySetResult();await Fails(async()=>await deniedOld);
    Check(client.Snapshot.SelectedChannel==channel&&client.Snapshot.Conversation!.Thread!.RootMessageId==other.RootMessageId&&client.Snapshot.Error is null);
    server.Hold("load_thread");var leaving=client.LoadThreadAsync(channel,root,view,user,stamp);await server.Held.Task.WaitAsync(TimeSpan.FromSeconds(8));await client.SelectChannelAsync(ConversationServer.B);server.Release.TrySetResult();await Fails(async()=>await leaving);Check(client.Snapshot.SelectedChannel==ConversationServer.B&&client.Snapshot.Conversation!.Thread is null&&client.Snapshot.Conversation.Reactions.Count==0);
    await Fails(async()=>await client.ToggleReactionAsync(channel,root,"👍","stale",view,user,stamp));Check(server.Calls["toggle_message_reaction"]==1);
}
await using(var server=await ConversationServer.StartAsync())
{
    // A root discovered through the thread directory is not a contiguous
    // history page and must not consume/root a hole in the channel window.
    var owner=new NativeAuthSession(server.SocketUri);await using var client=new NativeChatClient(owner);await client.StartAsync();long view=client.Snapshot.History!.ViewGeneration;
    var cold=ConversationServer.Message(99);server.Messages[cold.id]=cold;
    await client.LoadThreadAsync(ConversationServer.A,cold.id,view,ConversationServer.User,stamp);Check(client.Snapshot.Messages.All(m=>m.Id!=cold.id));
    await client.ReadMessageAsync(ConversationServer.A,cold.id,cold.id,view,ConversationServer.User,stamp);Check(client.Snapshot.Messages.All(m=>m.Id!=cold.id));
    await client.EditMessageAsync(ConversationServer.A,cold.id,"Cold edit","cold-edit",view,ConversationServer.User,stamp);Check(client.Snapshot.Messages.All(m=>m.Id!=cold.id)&&client.Snapshot.Conversation!.Thread!.Root!.Body=="Cold edit");
    server.Hold("load_thread");var delayedReadback=client.ReadMessageAsync(ConversationServer.A,cold.id,cold.id,view,ConversationServer.User,stamp);await server.Held.Task.WaitAsync(TimeSpan.FromSeconds(8));
    await server.MessageEvent(cold with{body="",deleted_at="2026-11-04T17:00:00Z"},"message_deleted");
    await Until(()=>client.Snapshot.Conversation!.Thread!.Root!.DeletedAt is not null);Check(client.Snapshot.Messages.All(m=>m.Id!=cold.id));
    server.Release.TrySetResult();var fresh=await delayedReadback;Check(fresh!.DeletedAt is not null&&fresh.Body=="");
}
await using(var server=await ConversationServer.StartAsync())
{
    var owner=new NativeAuthSession(server.SocketUri);await using var client=new NativeChatClient(owner);await client.StartAsync();long view=client.Snapshot.History!.ViewGeneration;
    for(ulong seq=10;seq<2011;seq++)server.Messages[ConversationServer.Id(seq)]=ConversationServer.Message(seq,1);
    var capped=await client.LoadThreadAsync(ConversationServer.A,ConversationServer.Id(1),view,ConversationServer.User,stamp);Check(capped.AtCapacity&&capped.Error is not null&&capped.Replies.Count==0);
    owner.RequireAuthentication();Check(client.Snapshot.Conversation!.Thread is null&&client.Snapshot.Conversation.Reactions.Count==0);
}
await using(var server=await ConversationServer.StartAsync())
{
    var owner=new NativeAuthSession(server.SocketUri);await using var client=new NativeChatClient(owner);await client.StartAsync();long view=client.Snapshot.History!.ViewGeneration;
    server.OversizeNextThread=true;var capacity=await client.LoadThreadAsync(ConversationServer.A,ConversationServer.Id(1),view,ConversationServer.User,stamp);
    Check(capacity.AtCapacity&&capacity.Error!.Contains("20 MiB")&&client.Snapshot.Connected&&client.Snapshot.History.ViewGeneration==view);
    await Task.Delay(150);Check(server.Calls["load_thread"]==1&&server.Calls["hello"]==1);
    var retry=await client.LoadThreadAsync(ConversationServer.A,ConversationServer.Id(1),view,ConversationServer.User,stamp);Check(!retry.AtCapacity&&retry.Replies.Count==2&&server.Calls["hello"]==1);
}
await using(var server=await ConversationServer.StartAsync())
{
    var owner=new NativeAuthSession(server.SocketUri);await using var client=new NativeChatClient(owner);await client.StartAsync();long view=client.Snapshot.History!.ViewGeneration;
    int admissions=0;owner.BeforeAdmission=()=>{if(++admissions==2)owner.RequireAuthentication();};
    await Fails(async()=>await client.EditMessageAsync(ConversationServer.A,ConversationServer.Id(1),"Blocked before write","blocked",view,ConversationServer.User,stamp));
    Check(!server.Calls.ContainsKey("edit_message")&&client.Snapshot.Messages.Count==0);
}
await using(var server=await ConversationServer.StartAsync())
{
    var owner=new NativeAuthSession(server.SocketUri);await using var client=new NativeChatClient(owner);await client.StartAsync();long view=client.Snapshot.History!.ViewGeneration;
    server.DenyType="load_thread";await Fails(async()=>await client.LoadThreadAsync(ConversationServer.A,ConversationServer.Id(1),view,ConversationServer.User,stamp));
    Check(client.Snapshot.SelectedChannel is null&&client.Snapshot.Messages.Count==0&&client.Snapshot.Conversation!.Thread is null);
}
await using(var server=await ConversationServer.StartAsync())
{
    var owner=new NativeAuthSession(server.SocketUri);await using var client=new NativeChatClient(owner);await client.StartAsync();long view=client.Snapshot.History!.ViewGeneration;
    server.DropType="toggle_message_reaction";await Fails(async()=>await client.ToggleReactionAsync(ConversationServer.A,ConversationServer.Id(1),"👍","uncertain-toggle",view,ConversationServer.User,stamp));
    await Until(()=>client.Snapshot.Connected&&!client.Snapshot.Loading);Check(server.Calls["toggle_message_reaction"]==1);
}
string receipt=args.Single();Directory.CreateDirectory(Path.GetDirectoryName(Path.GetFullPath(receipt))!);
await File.WriteAllTextAsync(receipt,JsonSerializer.Serialize(new{passed=true,production_transport_source=true,local_websocket=true,synthetic_session_owner=true,live_canary=false,threads_roots_replies_isolated=true,thread_generation_fence=true,read_ack_only=true,measured_reply_ids_only=true,stale_pages_preserve_tombstones_edits=true,mutation_ack_resource_validation=true,authoritative_readback_without_selection=true,reaction_snapshot_live_fence=true,duplicate_reaction_event_idempotent=true,uncertain_toggle_never_replayed=true,channel_account_scope_clear=true,thread_capacity_atomic=true,cold_thread_root_preserves_contiguous_history=true,readback_preserves_later_live_tombstone=true,oversized_frame_drained_without_reconnect=true,oversized_legal_thread_reports_capacity=true,send_parent_and_normalized_body_ack_fenced=true,account_revoked_before_mutation_write=true},new JsonSerializerOptions{WriteIndented=true}));
Console.WriteLine("PASS native thread, reaction, edit/delete conversation contracts");
