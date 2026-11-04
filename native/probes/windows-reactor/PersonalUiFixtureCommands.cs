namespace Sproyt.WindowsProbe;

// Personal-feature projections for the actual WinUI fixture. These delegates
// reference synthetic in-process state only; no socket, user file or OS UI.
internal sealed class NativePersonalUiFixtureCommands(Func<NativeAuthSnapshot> auth,
    Func<IReadOnlyList<NativeChannel>> channels, Action<NativeChannel> addChannel,
    Func<IReadOnlyList<NativeMessage>> messages) : IPersonalCommands
{
    internal const string PersonOneId = "00000000-0000-0000-0000-000000000081";
    internal const string PersonTwoId = "00000000-0000-0000-0000-000000000082";
    internal const string TaskId = "00000000-0000-0000-0000-000000000083";
    internal const string UnknownTaskId = "00000000-0000-0000-0000-000000000084";
    private NativePerson? _own;
    private readonly HashSet<string> _read = [];
    private readonly Dictionary<string, bool> _done = [];
    internal int DirectoryReads { get; private set; }
    internal int ChannelRefreshes { get; private set; }
    internal int DirectExecutions { get; private set; }
    internal int ProfileWrites { get; private set; }
    internal int StatusWrites { get; private set; }
    internal int MentionWrites { get; private set; }
    internal int TaskWrites { get; private set; }
    internal string? LastDirectUserId { get; private set; }
    internal bool FailNextProfileAfterCommit { get; set; }
    internal bool FailNextStatusAfterCommit { get; set; }
    internal bool FailNextDirectAfterCommit { get; set; }
    internal bool FailNextMentionAfterCommit { get; set; }
    internal bool FailNextTaskAfterCommit { get; set; }
    internal NativePerson OwnProfile => _own ??= new(auth().UserId ?? throw new InvalidOperationException("Fixture account missing."),
        "human", "fixture-self", auth().DisplayName ?? "UI-konto", "", "", null);
    internal bool MentionRead(string id) => _read.Contains(id);
    internal bool TaskDone(string id) => _done.GetValueOrDefault(id);
    internal void Reset()
    {
        _own = null; _read.Clear(); _done.Clear(); LastDirectUserId = null;
        DirectoryReads = ChannelRefreshes = DirectExecutions = ProfileWrites = StatusWrites = MentionWrites = TaskWrites = 0;
        FailNextProfileAfterCommit = FailNextStatusAfterCommit = FailNextDirectAfterCommit = FailNextMentionAfterCommit = FailNextTaskAfterCommit = false;
    }
    private void Admit(NativePersonalScope scope, CancellationToken cancellation)
    {
        cancellation.ThrowIfCancellationRequested();
        var current = auth();
        if (current.UserId != scope.UserId || current.SessionStamp != scope.SessionStamp || current.Phase != NativeAuthPhase.Active)
            throw new OperationCanceledException();
    }
    public Task<IReadOnlyList<NativePerson>> ListUsersAsync(NativePersonalScope scope, CancellationToken cancellationToken)
    {
        Admit(scope, cancellationToken); DirectoryReads++;
        return Task.FromResult<IReadOnlyList<NativePerson>>([
            new(PersonOneId, "human", "anna-ein", "Anna", "På tur", "🌿", null),
            OwnProfile,
            new(PersonTwoId, "human", "anna-to", "Anna", "", "", null)]);
    }
    public Task<NativeDirectResult> OpenDirectAsync(NativePersonalScope scope, string userId, string requestId, CancellationToken cancellationToken)
    {
        Admit(scope, cancellationToken);
        if (userId is not (PersonOneId or PersonTwoId)) throw NativeCommandException.FromServer(requestId, "not_found", "Fixture person missing.");
        DirectExecutions++; LastDirectUserId = userId;
        string channel = channels().FirstOrDefault(item => item.IsDirect && item.DirectUserId == userId)?.Id ?? "personal-dm-" + userId[^2..];
        if (channels().All(item => item.Id != channel))
            addChannel(new(channel, "Anna", Kind: "private", IsDirect: true, Role: "member", DirectUserId: userId));
        if (FailNextDirectAfterCommit) { FailNextDirectAfterCommit = false; throw NativeCommandException.Uncertain(requestId); }
        return Task.FromResult(new NativeDirectResult(channel));
    }
    public Task<IReadOnlyList<NativeChannel>> RefreshPersonalChannelsAsync(NativePersonalScope scope, CancellationToken cancellationToken)
    { Admit(scope, cancellationToken); ChannelRefreshes++; return Task.FromResult(channels()); }
    public Task<NativePerson> UpdateProfileAsync(NativePersonalScope scope, string displayName, string requestId, CancellationToken cancellationToken)
    {
        Admit(scope, cancellationToken); ProfileWrites++; _own = OwnProfile with { DisplayName = displayName.Trim() };
        if (FailNextProfileAfterCommit) { FailNextProfileAfterCommit = false; throw NativeCommandException.Uncertain(requestId); }
        return Task.FromResult(_own);
    }
    public Task<NativePerson> SetStatusAsync(NativePersonalScope scope, string text, string emoji, string requestId, CancellationToken cancellationToken)
    {
        Admit(scope, cancellationToken); StatusWrites++; _own = OwnProfile with { StatusText = text.Trim(), StatusEmoji = emoji.Trim(), StatusExpiresAt = null };
        if (FailNextStatusAfterCommit) { FailNextStatusAfterCommit = false; throw NativeCommandException.Uncertain(requestId); }
        return Task.FromResult(_own);
    }
    public Task<IReadOnlyList<NativeMention>> ListMentionsAsync(NativePersonalScope scope, CancellationToken cancellationToken)
    {
        Admit(scope, cancellationToken);
        return Task.FromResult<IReadOnlyList<NativeMention>>(messages().Where(message => message.Id is "conversation-root-one" or "conversation-reply-one")
            .Select(message => new NativeMention(message, "Prat", _read.Contains(message.Id))).ToArray());
    }
    public Task<string> MarkMentionReadAsync(NativePersonalScope scope, string messageId, string requestId, CancellationToken cancellationToken)
    {
        Admit(scope, cancellationToken);
        if (messages().All(message => message.Id != messageId)) throw NativeCommandException.FromServer(requestId, "not_found", "Fixture mention missing.");
        MentionWrites++; _read.Add(messageId);
        if (FailNextMentionAfterCommit) { FailNextMentionAfterCommit = false; throw NativeCommandException.Uncertain(requestId); }
        return Task.FromResult(messageId);
    }
    public Task<IReadOnlyList<NativeAssignedTask>> ListTasksAsync(NativePersonalScope scope, CancellationToken cancellationToken)
    {
        Admit(scope, cancellationToken);
        return Task.FromResult<IReadOnlyList<NativeAssignedTask>>([
            AssignedTask(TaskId, "conversation-reply-one", "Oppgåve frå svaret", scope.UserId),
            AssignedTask(UnknownTaskId, "fixture-unloaded-source", "Oppgåve frå eldre historie", scope.UserId)]);
    }
    private NativeAssignedTask AssignedTask(string id, string source, string title, string user) =>
        new(id, source, "channel-a", "Prat", user, title, _done.GetValueOrDefault(id) ? "done" : "open", _done.GetValueOrDefault(id) ? DateTimeOffset.UtcNow : null);
    public Task<NativeAssignedTask> SetTaskDoneAsync(NativePersonalScope scope, string taskId, bool done, string requestId, CancellationToken cancellationToken)
    {
        Admit(scope, cancellationToken);
        if (taskId is not (TaskId or UnknownTaskId)) throw NativeCommandException.FromServer(requestId, "not_found", "Fixture task missing.");
        TaskWrites++; _done[taskId] = done;
        if (FailNextTaskAfterCommit) { FailNextTaskAfterCommit = false; throw NativeCommandException.Uncertain(requestId); }
        return Task.FromResult(AssignedTask(taskId, taskId == TaskId ? "conversation-reply-one" : "fixture-unloaded-source",
            taskId == TaskId ? "Oppgåve frå svaret" : "Oppgåve frå eldre historie", scope.UserId));
    }
}
