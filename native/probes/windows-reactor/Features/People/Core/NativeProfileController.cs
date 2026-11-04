namespace Sproyt.WindowsProbe;

internal sealed class NativeProfileController(IPersonalCommands commands) : NativePersonalController
{
    private sealed record WriteIntent(bool Status, string RequestId, string Name, string Text, string Emoji, long Revision, bool Checked = false);
    private NativePerson? _profile;
    private string _name = "", _text = "", _emoji = "";
    private bool _nameDirty, _statusDirty, _loaded;
    private long _nameRevision, _statusRevision;
    private string? _error, _notice;
    private WriteIntent? _write;
    protected override void ResetOwner()
    {
        _name = _text = _emoji = ""; _nameDirty = _statusDirty = false;
        _nameRevision = _statusRevision = 0; _write = null;
    }
    protected override void ResetProjection() { _profile = null; _loaded = false; _error = _notice = null; }
    private bool CanWrite => Scope is not null && _loaded && Active is null && (_write is null || _write.Checked);
    internal NativeProfileSnapshot Capture()
    {
        lock (Gate)
        {
            if (Scope is null) return new(null, null, "", "", "", false, false, false, false, false, null, null);
            return new(Scope, _profile, _name, _text, _emoji, Active?.Kind == "profile-load", _loaded,
                Active?.Kind == "profile-write", _write is not null && Active?.Kind != "profile-write", CanWrite, _error, _notice);
        }
    }
    internal void EditName(NativePersonalScope scope, string value)
    {
        lock (Gate) { if (!Matches(scope)) return; _name = value; _nameDirty = _profile is null || value != _profile.DisplayName; _nameRevision++; _notice = null; }
        Notify();
    }
    internal void EditStatusText(NativePersonalScope scope, string value)
    {
        lock (Gate) { if (!Matches(scope)) return; _text = value; _statusDirty = _profile is null || _text != _profile.StatusText || _emoji != _profile.StatusEmoji; _statusRevision++; _notice = null; }
        Notify();
    }
    internal void EditStatusEmoji(NativePersonalScope scope, string value)
    {
        lock (Gate) { if (!Matches(scope)) return; _emoji = value; _statusDirty = _profile is null || _text != _profile.StatusText || _emoji != _profile.StatusEmoji; _statusRevision++; _notice = null; }
        Notify();
    }
    internal Task LoadAsync(NativePersonalScope scope) => ReadAsync(scope);
    internal Task ReadBackAsync(NativePersonalScope scope) => ReadAsync(scope);
    private async Task ReadAsync(NativePersonalScope scope)
    {
        Operation? operation;
        lock (Gate) { operation = Begin(scope, "profile-load"); if (operation is null) return; _error = null; }
        Notify();
        try
        {
            var people = await commands.ListUsersAsync(scope, operation.Token);
            var own = people.SingleOrDefault(person => person.Id == scope.UserId) ?? throw new InvalidDataException();
            lock (Gate)
            {
                if (!Current(operation)) return;
                ApplyProfile(own, null); _loaded = true;
                if (_write is { } uncertain)
                {
                    bool confirmed = uncertain.Status ? own.StatusText == uncertain.Text && own.StatusEmoji == uncertain.Emoji : own.DisplayName == uncertain.Name;
                    if (confirmed) { _write = null; _notice = "Lagringa er stadfesta av serveren."; }
                    else
                    {
                        _write = uncertain with { Checked = true };
                        _notice = "Serveren viser andre verdiar. Utkastet er bevart. Ingen endring er send på nytt av kontrollen.";
                    }
                }
            }
        }
        catch (OperationCanceledException) { lock (Gate) { if (Current(operation)) _error = "Henting av profilen vart avbroten. Prøv igjen."; } }
        catch { lock (Gate) { if (Current(operation)) _error = "Din eigen profil kunne ikkje hentast. Prøv igjen."; } }
        finally { Finish(operation); }
    }
    internal Task SaveNameAsync(NativePersonalScope scope) => SaveAsync(scope, status: false);
    internal Task SaveStatusAsync(NativePersonalScope scope) => SaveAsync(scope, status: true);
    internal async Task ClearStatusAsync(NativePersonalScope scope)
    {
        lock (Gate)
        {
            if (!Matches(scope) || !CanWrite) return;
            _text = _emoji = ""; _statusDirty = true; _statusRevision++;
        }
        await SaveAsync(scope, status: true);
    }
    private async Task SaveAsync(NativePersonalScope scope, bool status)
    {
        Operation? operation; WriteIntent write;
        lock (Gate)
        {
            if (!Matches(scope) || !CanWrite) return;
            string name = _name.Trim(), text = _text.Trim(), emoji = _emoji.Trim();
            if (!status && name.Length == 0) { _error = "Skriv eit visingsnamn før du lagrar."; operation = null; write = null!; }
            else
            {
                operation = Begin(scope, "profile-write"); if (operation is null) return;
                _write = write = new(status, Guid.NewGuid().ToString("D"), name, text, emoji, status ? _statusRevision : _nameRevision);
                _error = _notice = null;
            }
        }
        Notify(); if (operation is null) return;
        try
        {
            var receipt = status
                ? await commands.SetStatusAsync(scope, write.Text, write.Emoji, write.RequestId, operation.Token)
                : await commands.UpdateProfileAsync(scope, write.Name, write.RequestId, operation.Token);
            if (receipt.Id != scope.UserId || (status ? receipt.StatusText != write.Text || receipt.StatusEmoji != write.Emoji : receipt.DisplayName != write.Name))
                throw new InvalidDataException();
            lock (Gate)
            {
                if (!Current(operation)) return;
                ApplyProfile(receipt, write); _write = null;
                _notice = status ? "Statusen er lagra." : "Namnet er lagra.";
            }
        }
        catch (Exception error)
        {
            lock (Gate)
            {
                if (Current(operation))
                {
                    if (Rejected(error, write.RequestId)) { _write = null; _error = "Endringa vart avvist. Verdiane dine er bevarte."; }
                    else _error = "Vi fekk ikkje stadfesta lagringa. Kontroller servertilstanden før du prøver igjen.";
                }
            }
        }
        finally { Finish(operation); }
    }
    private void ApplyProfile(NativePerson profile, WriteIntent? receipt)
    {
        _profile = profile;
        if (!_nameDirty || receipt is { Status: false } && receipt.Revision == _nameRevision) _name = profile.DisplayName;
        if (!_statusDirty || receipt is { Status: true } && receipt.Revision == _statusRevision)
        { _text = profile.StatusText; _emoji = profile.StatusEmoji; }
        _nameDirty = _name != profile.DisplayName;
        _statusDirty = _text != profile.StatusText || _emoji != profile.StatusEmoji;
    }
}
