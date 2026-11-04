// A surface visit is owned by the UI; transport admission uses the exact user
// and full session stamp. A refreshed token may preserve dirty fields locally.
internal sealed record NativePersonalScope(string UserId, string SessionStamp, long Visit);

internal sealed record NativePerson(string Id, string Kind, string? Handle, string DisplayName,
    string StatusText, string StatusEmoji, DateTimeOffset? StatusExpiresAt);
internal sealed record NativeMention(NativeMessage Message, string ChannelName, bool Read);
internal sealed record NativeAssignedTask(string Id, string SourceMessageId, string ChannelId, string ChannelName,
    string AssigneeId, string Title, string Status, DateTimeOffset? CompletedAt);
internal sealed record NativeDirectResult(string ChannelId);

internal interface IPersonalCommands
{
    Task<IReadOnlyList<NativePerson>> ListUsersAsync(NativePersonalScope scope, CancellationToken cancellationToken);
    Task<NativeDirectResult> OpenDirectAsync(NativePersonalScope scope, string userId, string requestId, CancellationToken cancellationToken);
    Task<IReadOnlyList<NativeChannel>> RefreshPersonalChannelsAsync(NativePersonalScope scope, CancellationToken cancellationToken);
    Task<IReadOnlyList<NativeMention>> ListMentionsAsync(NativePersonalScope scope, CancellationToken cancellationToken);
    Task<string> MarkMentionReadAsync(NativePersonalScope scope, string messageId, string requestId, CancellationToken cancellationToken);
    Task<IReadOnlyList<NativeAssignedTask>> ListTasksAsync(NativePersonalScope scope, CancellationToken cancellationToken);
    Task<NativeAssignedTask> SetTaskDoneAsync(NativePersonalScope scope, string taskId, bool done, string requestId, CancellationToken cancellationToken);
    Task<NativePerson> UpdateProfileAsync(NativePersonalScope scope, string displayName, string requestId, CancellationToken cancellationToken);
    Task<NativePerson> SetStatusAsync(NativePersonalScope scope, string text, string emoji, string requestId, CancellationToken cancellationToken);
}
