namespace Sproyt.WindowsProbe;

// Presentation depends on this credential-free application boundary. Feature
// modules share its snapshot and actions instead of creating transport owners.
internal sealed record LiveSnapshot(NativeAuthSnapshot Authentication, NativeChatSnapshot Chat,
    WorkspaceSnapshot? Workspace, bool ShowConversation, string? Error, bool ConfirmPendingSends = false)
{
    public int RejectedSends => Workspace?.Outbox.Count(item => item.State == "rejected") ?? 0;
    public int PendingSends => Workspace?.Outbox.Count(item => item.State != "rejected") ?? 0;
}

internal interface ILiveApplication : IAsyncDisposable
{
    event Action? Changed;
    NativeAuthSnapshot Authentication { get; }
    NativeChatSnapshot Chat { get; }
    WorkspaceSnapshot? Workspace { get; }
    NativeMediaLibrary? Media { get; }
    LiveSnapshot Capture();
    Task LoginAsync();
    Task LogoutAsync();
    Task ResumeAsync();
    Task SelectAsync(string channel);
    void Edit(string channel, string body);
    Task SendAsync(string channel);
    Task ConfirmPendingSendsAsync(string expectedUser, string expectedStamp);
    Task DiscardRejectedAsync(string expectedUser, string expectedStamp, string requestId);
}
