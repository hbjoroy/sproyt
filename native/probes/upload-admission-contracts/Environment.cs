using System.Net;
using System.Text;

namespace Sproyt.WindowsProbe;

// Only this headless executable supplies these synthetic platform boundaries.
// The application, queue, transport adapter and scope-registration code above
// are linked production source. Disk/Rust/DPAPI acceptance lives in its own probe.
internal enum NativeAuthPhase { Active, Offline, SignedOut }
internal sealed record NativeAuthSnapshot(NativeAuthPhase Phase, string? UserId, string? SessionStamp);
internal sealed record NativeMediaScope(string UserId, string FamilyStamp);
internal sealed record NativeMessageActionScope(string UserId, string SessionStamp, string ChannelId, string? ThreadRootId = null);
internal sealed record ComposerTarget(string ChannelId, string? ParentMessageId) { internal string Key => ChannelId + "/" + ParentMessageId; }
internal sealed class NativeProbeException(string code) : IOException(code) { internal string Code { get; } = code; }
internal sealed class NativeAuthSession
{
    private readonly object _gate = new();
    private NativeAuthSnapshot _state = new(NativeAuthPhase.Active, Probe.User, "1:family-a:1");
    internal NativeAuthSnapshot Snapshot { get { lock (_gate) return _state; } }
    internal T ProjectSnapshot<T>(Func<NativeAuthSnapshot,T> projection) { lock (_gate) return projection(_state); }
    internal void Set(NativeAuthPhase phase, string? user, string? stamp) { lock (_gate) _state = new(phase,user,stamp); }
    internal Task<HttpResponseMessage> SendUploadAsync(HttpRequestMessage request, NativeMediaScope scope, CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        // No network runs in this harness. A disabled capability keeps queued
        // file identity observable without testing another transport simulation.
        return Task.FromResult(new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent("{\"enabled\":false,\"chunk_bytes\":1048576,\"max_file_bytes\":4194304}",Encoding.UTF8,"application/json") });
    }
}
internal sealed record WorkspaceState(bool Ready = true);
internal sealed class ChatWorkspace
{
    internal WorkspaceState State { get; } = new();
    internal string? AttachFailure;
    internal string? SettleFailure, ExpectedReceipt;
    internal bool HoldSettle, ReceiptSettled;
    internal TaskCompletionSource SettleEntered { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
    internal TaskCompletionSource SettleRelease { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
    internal void EnsureChannels(IReadOnlyList<string> channels) { }
    internal Task AttachUploadAsync(string key,string job,string marker) => AttachFailure is null ? Task.CompletedTask : Task.FromException(new NativeProbeException(AttachFailure));
    internal Task ReconcileUploadReceiptsAsync(IReadOnlyList<string> jobs)
    {
        if(ExpectedReceipt is string held&&!ReceiptSettled&&!jobs.Contains(held))throw new IOException("Unsettled receipt collected before handoff resolution");
        return Task.CompletedTask;
    }
    internal async Task SettleUploadAsync(string jobId)
    {
        if(ExpectedReceipt is not null&&ExpectedReceipt!=jobId)throw new IOException("Wrong handoff receipt");
        SettleEntered.TrySetResult();if(HoldSettle)await SettleRelease.Task;
        if(SettleFailure is string failure)throw new NativeProbeException(failure);
        ReceiptSettled=true;
    }
    internal void RemoveDraftAttachment(string key,int index,string marker) { }
}
internal sealed record Channel(string Id);
internal sealed record ChatState(string UserId, IReadOnlyList<Channel> Channels);
internal sealed class NativeUploadDiskStore(string directory,string account,Func<NativeUploadTarget,string,string,Task<bool>> attach,Func<IReadOnlyList<string>,Task> reconcile) : INativeUploadLocalStore, IDisposable
{
    private NativeUploadJob[] _jobs = [];
    internal static NativeUploadJob[] NextLoadJobs = [];
    internal int ActiveStaging;
    internal bool Disposed;
    public Task<IReadOnlyList<NativeUploadJob>> LoadAsync(CancellationToken cancellationToken)
    {if(string.IsNullOrEmpty(directory)||string.IsNullOrEmpty(account))throw new IOException("Invalid synthetic store owner");_jobs=Interlocked.Exchange(ref NextLoadJobs,[]);return Task.FromResult<IReadOnlyList<NativeUploadJob>>(_jobs);}
    public async Task SaveAsync(IReadOnlyList<NativeUploadJob> jobs,CancellationToken cancellationToken)
    { cancellationToken.ThrowIfCancellationRequested(); _jobs=jobs.ToArray();await reconcile(jobs.Select(job=>job.Id).ToArray()); }
    internal Task<NativeUploadSource> StageFileAsync(string path,CancellationToken cancellationToken) => StageAsync(new MemoryStream([1]),path,"text/plain",cancellationToken);
    internal async Task<NativeUploadSource> StageAsync(Stream stream,string name,string type,CancellationToken cancellationToken)
    {
        if(Disposed)throw new ObjectDisposedException(nameof(NativeUploadDiskStore));
        Interlocked.Increment(ref ActiveStaging);
        try
        {
            byte[] buffer=new byte[1];long count=0;
            while(await stream.ReadAsync(buffer,cancellationToken)!=0)count++;
            cancellationToken.ThrowIfCancellationRequested();
            return new(Guid.NewGuid().ToString("D"),name,type,count,new string('0',64));
        }
        finally {Interlocked.Decrement(ref ActiveStaging);}
    }
    internal Task ReleaseStagedAsync(string source) => Task.CompletedTask;
    public Task<Stream> OpenReadAsync(NativeUploadSource source,CancellationToken cancellationToken) => Task.FromResult<Stream>(new MemoryStream([1]));
    public Task<bool> AttachAsync(NativeUploadTarget target,NativeUploadedAttachment attachment,string job,CancellationToken cancellationToken) => attach(target,job,attachment.Marker);
    public void Dispose(){if(ActiveStaging!=0)throw new InvalidOperationException("Disk disposed before staging released admission");Disposed=true;}
}
internal sealed partial class LiveApplication
{
    private readonly NativeAuthSession _authentication = new();
    private readonly CancellationTokenSource _lifetime = new();
    private bool _closing;
    private string _account = Probe.User;
    private readonly string _directory = "unused-synthetic-store";
    private readonly ChatWorkspace _workspace = new();
    private string _selected = Probe.Channel;
    private NativeAuthSnapshot Authentication => _authentication.Snapshot;
    private ChatState Chat => new(_account,[new(Probe.Channel),new(Probe.OtherChannel)]);
    private void Notify() { }
    private static string? SendFamily(string? stamp) => stamp?.LastIndexOf(':') is int at && at>0 ? stamp[..at] : null;
    private bool CurrentActionScope(NativeMessageActionScope scope) => scope.UserId==Authentication.UserId&&scope.SessionStamp==Authentication.SessionStamp&&scope.ChannelId==_selected;
    private void WithComposer(NativeMessageActionScope scope,Action<ChatWorkspace,string> action) => action(_workspace,scope.ChannelId);
    internal Task OpenForProbeAsync() => InitializeUploadsAsync(_account,_workspace);
    internal void ScopeForProbe(NativeAuthPhase phase,string? user,string? stamp)
    { _authentication.Set(phase,user,stamp);if(user is not null)_account=user;BindUploadScope(); }
    internal void NavigateForProbe() => _selected=Probe.OtherChannel;
    internal NativeUploadTarget TargetForProbe() => CaptureUploadTarget(new(Probe.User,"1:family-a:1",Probe.Channel))!;
    internal NativeUploadSnapshot JobsForProbe() => _uploads!.Capture();
    internal async Task CloseForProbeAsync(){_closing=true;await CloseUploadsAsync();_lifetime.Dispose();}
    internal Task<bool> AttachForProbeAsync(string failure)
    { _workspace.AttachFailure=failure;return AttachCompletedUploadAsync(_workspace,new(Probe.User,"1:family-a",Probe.Channel),"job","marker"); }
    internal Task OpenHeldHandoffForProbeAsync(NativeUploadJob job,bool hold=false,string? failure=null)
    {
        NativeUploadDiskStore.NextLoadJobs=[job];_workspace.ExpectedReceipt=job.Id;_workspace.HoldSettle=hold;_workspace.SettleFailure=failure;
        return OpenForProbeAsync();
    }
    internal Task SettlementEnteredForProbe()=>_workspace.SettleEntered.Task;
    internal void ReleaseSettlementForProbe()=>_workspace.SettleRelease.TrySetResult();
    internal bool ReceiptSettledForProbe()=>_workspace.ReceiptSettled;
    internal bool SourceRetainedForProbe(string id)=>_uploads!.RetainsSource(id);
}
