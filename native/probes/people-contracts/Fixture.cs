// Only the external chat DTOs needed to compile IPersonalCommands are represented
// here. Production controllers, personal DTOs and command errors are linked above;
// real wire parsing/admission is covered by the personal transport contracts.
internal sealed record NativeChannel(string Id, string Name, bool IsDirect = false, string? DirectUserId = null);
internal sealed record NativeMessage(string Id);

internal sealed class Commands : IPersonalCommands
{
    internal NativePerson Self = new("self", "human", "self", "Original", "Ready", "🙂", null);
    internal NativePerson Other = new("other", "human", "anna", "Anna", "", "", null);
    internal Func<NativePersonalScope, CancellationToken, Task<IReadOnlyList<NativePerson>>>? Users;
    internal Func<NativePersonalScope, string, string, CancellationToken, Task<NativeDirectResult>>? Open;
    internal Func<NativePersonalScope, CancellationToken, Task<IReadOnlyList<NativeChannel>>>? Channels;
    internal Func<NativePersonalScope, string, string, CancellationToken, Task<NativePerson>>? Name;
    internal Func<NativePersonalScope, string, string, string, CancellationToken, Task<NativePerson>>? Status;
    internal int UserReads, ChannelReads, DirectWrites, NameWrites, StatusWrites;
    internal List<string> RequestIds = [];
    public Task<IReadOnlyList<NativePerson>> ListUsersAsync(NativePersonalScope scope, CancellationToken cancellationToken)
    { UserReads++; return Users?.Invoke(scope, cancellationToken) ?? Task.FromResult<IReadOnlyList<NativePerson>>([Other, Self]); }
    public Task<NativeDirectResult> OpenDirectAsync(NativePersonalScope scope, string userId, string requestId, CancellationToken cancellationToken)
    { DirectWrites++; RequestIds.Add(requestId); return Open?.Invoke(scope, userId, requestId, cancellationToken) ?? Task.FromResult(new NativeDirectResult("dm")); }
    public Task<IReadOnlyList<NativeChannel>> RefreshPersonalChannelsAsync(NativePersonalScope scope, CancellationToken cancellationToken)
    { ChannelReads++; return Channels?.Invoke(scope, cancellationToken) ?? Task.FromResult<IReadOnlyList<NativeChannel>>([new("dm", "Anna", true, "other")]); }
    public Task<NativePerson> UpdateProfileAsync(NativePersonalScope scope, string displayName, string requestId, CancellationToken cancellationToken)
    { NameWrites++; RequestIds.Add(requestId); if (Name is not null) return Name(scope, displayName, requestId, cancellationToken); Self = Self with { DisplayName = displayName }; return Task.FromResult(Self); }
    public Task<NativePerson> SetStatusAsync(NativePersonalScope scope, string text, string emoji, string requestId, CancellationToken cancellationToken)
    { StatusWrites++; RequestIds.Add(requestId); if (Status is not null) return Status(scope, text, emoji, requestId, cancellationToken); Self = Self with { StatusText = text, StatusEmoji = emoji, StatusExpiresAt = null }; return Task.FromResult(Self); }
    public Task<IReadOnlyList<NativeMention>> ListMentionsAsync(NativePersonalScope scope, CancellationToken cancellationToken) => throw new NotSupportedException();
    public Task<string> MarkMentionReadAsync(NativePersonalScope scope, string messageId, string requestId, CancellationToken cancellationToken) => throw new NotSupportedException();
    public Task<IReadOnlyList<NativeAssignedTask>> ListTasksAsync(NativePersonalScope scope, CancellationToken cancellationToken) => throw new NotSupportedException();
    public Task<NativeAssignedTask> SetTaskDoneAsync(NativePersonalScope scope, string taskId, bool done, string requestId, CancellationToken cancellationToken) => throw new NotSupportedException();
}
