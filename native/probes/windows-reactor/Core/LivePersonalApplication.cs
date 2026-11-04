namespace Sproyt.WindowsProbe;

internal sealed partial class LiveApplication
{
    private NativePersonalScope? _personalCover;
    private long _personalCoverVisit;
    public IPersonalCommands PersonalCommands => _chat;
    public void CoverConversation(NativePersonalScope scope) => _authentication.ProjectSnapshot(auth =>
    {
        if (auth.UserId == scope.UserId && auth.SessionStamp == scope.SessionStamp && auth.UserId == _account && scope.Visit > _personalCoverVisit)
        { _personalCoverVisit = scope.Visit; Volatile.Write(ref _personalCover, scope); }
        return true;
    });
    public void UncoverConversation(NativePersonalScope expectedScope)
    {
        var captured = Volatile.Read(ref _personalCover);
        if (captured == expectedScope) Interlocked.CompareExchange(ref _personalCover, null, captured);
    }
    private bool ConversationCovered => _authentication.ProjectSnapshot(auth => Volatile.Read(ref _personalCover) is { } cover &&
        cover.UserId == auth.UserId && SameSendFamily(SendFamily(cover.SessionStamp), auth.SessionStamp));
}
