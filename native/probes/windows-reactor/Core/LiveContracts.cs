namespace Sproyt.WindowsProbe;

// Presentation depends on this credential-free application boundary. Feature
// modules share its snapshot and actions instead of creating transport owners.
internal sealed record LiveSnapshot(NativeAuthSnapshot Authentication, NativeChatSnapshot Chat,
    WorkspaceSnapshot? Workspace, bool ShowConversation, string? Error, bool ConfirmPendingSends = false,
    bool DeliveryRetryPending = false, NativeNavigationSnapshot? Navigation = null, NativeThreadFeatureSnapshot? Threads = null,
    NativeUploadSnapshot? Uploads = null, string? UploadError = null, NativeUploadTarget? UploadErrorTarget = null)
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
    IPersonalCommands PersonalCommands { get; }
    void CoverConversation(NativePersonalScope scope);
    void UncoverConversation(NativePersonalScope expectedScope);
    LiveSnapshot Capture();
    Task LoginAsync();
    Task LogoutAsync();
    Task ResumeAsync();
    Task RetryPendingSendsAsync(string expectedUser, string expectedStamp);
    Task SelectAsync(string channel);
    Task NavigateAsync(NativeNavigationTarget target);
    Task BackAsync();
    Task ForwardAsync();
    void SetNavigationQuery(string query);
    void SetNavigationScope(NativeNavigationDirectoryScope scope);
    void SetNavigationInteraction(bool interacting);
    void RememberViewport(NativeNavigationVisit visit, NativeScrollAnchor anchor);
    Task LoadOlderAsync(string channel, long viewGeneration, string expectedUser, string expectedStamp, NativeNavigationVisit visit);
    Task MarkVisibleReadAsync(string channel, long viewGeneration, IReadOnlyList<string> messageIds, string expectedUser, string expectedStamp, NativeNavigationVisit visit);
    Task OpenThreadDirectoryAsync(NativeMessageActionScope scope);
    Task RefreshConversationAsync(NativeMessageActionScope scope);
    Task CloseThreadAsync(NativeThreadVisit visit);
    Task RetryThreadAsync(NativeThreadVisit visit);
    void RememberThreadViewport(NativeThreadVisit visit, NativeScrollAnchor anchor);
    Task MarkThreadVisibleReadAsync(NativeThreadVisit visit, long viewGeneration, long threadGeneration, IReadOnlyList<string> messageIds);
    Task ExecuteMessageMutationAsync(NativeMessageMutation mutation);
    Task<bool?> ReadBackMessageMutationAsync(NativeMessageMutation mutation);
    void EditComposer(NativeMessageActionScope scope, string body);
    Task SendComposerAsync(NativeMessageActionScope scope);
    NativeUploadTarget? CaptureUploadTarget(NativeMessageActionScope scope);
    Task AddUploadFilesAsync(NativeUploadTarget target, IReadOnlyList<string> paths);
    Task<int> AddUploadFilesCountedAsync(NativeUploadTarget target, IReadOnlyList<string> paths);
    Task AddUploadStreamAsync(NativeUploadTarget target, Stream stream, string fileName, string contentType);
    void RemoveDraftAttachment(NativeMessageActionScope scope, int index, string expectedMarker);
    Task RetryUploadAsync(string expectedUser, string expectedStamp, string jobId);
    Task CancelUploadAsync(string expectedUser, string expectedStamp, string jobId);
    Task RemoveUploadAsync(string expectedUser, string expectedStamp, string jobId);
    void Edit(string channel, string body);
    Task SendAsync(string channel);
    Task ConfirmPendingSendsAsync(string expectedUser, string expectedStamp);
    Task DiscardRejectedAsync(string expectedUser, string expectedStamp, string requestId);
}
