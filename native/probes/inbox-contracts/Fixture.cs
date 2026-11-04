using Sproyt.WindowsProbe;

internal static class Data
{
    internal static readonly NativePersonalScope Scope = new("user-a", "1:family-a:1", 1);
    internal static readonly NativeChannel Channel = new("channel-a", "Prat", LastReadSequence: 3, LatestSequence: 8);
    internal static readonly NativeChannel OtherChannel = new("channel-b", "Idear", LastReadSequence: 0, LatestSequence: 2);
    internal static readonly NativeMessage Message = new("message-a", Channel.Id, "author", "Anna", "Ei omtale", DateTimeOffset.UnixEpoch, 7, "thread-root");
    internal static readonly NativeMention Mention = new(Message, "Prat", false);
    internal static readonly NativeAssignedTask Task = new("task-a", Message.Id, Channel.Id, "Prat", Scope.UserId, "Ei oppgåve", "open", null);
    internal static NativeInboxController Owner()
    {
        var owner = new NativeInboxController(); owner.Bind(Scope, [Channel, OtherChannel]); return owner;
    }
}
internal sealed class Commands : IPersonalCommands
{
    internal IReadOnlyList<NativeMention> Mentions = [Data.Mention];
    internal IReadOnlyList<NativeAssignedTask> Tasks = [Data.Task];
    internal IReadOnlyList<NativeChannel> Channels = [Data.Channel, Data.OtherChannel];
    internal Func<Task<IReadOnlyList<NativeMention>>>? OnMentions;
    internal Func<Task<IReadOnlyList<NativeAssignedTask>>>? OnTasks;
    internal Func<Task<IReadOnlyList<NativeChannel>>>? OnChannels;
    internal Func<string, Task<string>>? OnMark;
    internal Func<string, bool, Task<NativeAssignedTask>>? OnDone;
    internal int MentionReads, TaskReads, ChannelReads, MentionWrites, TaskWrites;
    internal readonly List<string> Requests = [];
    public Task<IReadOnlyList<NativeMention>> ListMentionsAsync(NativePersonalScope scope, CancellationToken cancellationToken)
    { MentionReads++; return OnMentions?.Invoke() ?? System.Threading.Tasks.Task.FromResult(Mentions); }
    public Task<IReadOnlyList<NativeAssignedTask>> ListTasksAsync(NativePersonalScope scope, CancellationToken cancellationToken)
    { TaskReads++; return OnTasks?.Invoke() ?? System.Threading.Tasks.Task.FromResult(Tasks); }
    public Task<IReadOnlyList<NativeChannel>> RefreshPersonalChannelsAsync(NativePersonalScope scope, CancellationToken cancellationToken)
    { ChannelReads++; return OnChannels?.Invoke() ?? System.Threading.Tasks.Task.FromResult(Channels); }
    public Task<string> MarkMentionReadAsync(NativePersonalScope scope, string messageId, string requestId, CancellationToken cancellationToken)
    { MentionWrites++; Requests.Add(requestId); return OnMark?.Invoke(messageId) ?? System.Threading.Tasks.Task.FromResult(messageId); }
    public Task<NativeAssignedTask> SetTaskDoneAsync(NativePersonalScope scope, string taskId, bool done, string requestId, CancellationToken cancellationToken)
    {
        TaskWrites++; Requests.Add(requestId);
        return OnDone?.Invoke(taskId, done) ?? System.Threading.Tasks.Task.FromResult(Tasks.Single(task => task.Id == taskId) with
        { Status = done ? "done" : "open", CompletedAt = done ? DateTimeOffset.UnixEpoch : null });
    }
    public Task<IReadOnlyList<NativePerson>> ListUsersAsync(NativePersonalScope scope, CancellationToken cancellationToken) => throw new NotSupportedException();
    public Task<NativeDirectResult> OpenDirectAsync(NativePersonalScope scope, string userId, string requestId, CancellationToken cancellationToken) => throw new NotSupportedException();
    public Task<NativePerson> UpdateProfileAsync(NativePersonalScope scope, string displayName, string requestId, CancellationToken cancellationToken) => throw new NotSupportedException();
    public Task<NativePerson> SetStatusAsync(NativePersonalScope scope, string text, string emoji, string requestId, CancellationToken cancellationToken) => throw new NotSupportedException();
}
