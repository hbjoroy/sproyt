namespace Sproyt.WindowsProbe;

internal sealed class NativePeopleController(IPersonalCommands commands) : NativePersonalController
{
    private sealed record DirectIntent(string UserId, string RequestId, string? ChannelId = null);
    private string _query = "";
    private NativePerson[] _people = [];
    private bool _loaded;
    private string? _readError, _openError;
    private DirectIntent? _direct;
    protected override void ResetOwner() { _query = ""; _direct = null; }
    protected override void ResetProjection() { _people = []; _loaded = false; _readError = _openError = null; }
    internal NativePeopleSnapshot Capture()
    {
        lock (Gate)
        {
            if (Scope is null) return new(null, "", [], false, false, null, null, null, false);
            string needle = _query.Trim();
            var visible = _people.Where(person => person.Id != Scope.UserId &&
                (needle.Length == 0 || person.DisplayName.Contains(needle, StringComparison.OrdinalIgnoreCase) ||
                 person.Handle?.Contains(needle.TrimStart('@'), StringComparison.OrdinalIgnoreCase) == true))
                .OrderBy(person => person.DisplayName, StringComparer.OrdinalIgnoreCase).ThenBy(person => person.Id, StringComparer.Ordinal).ToArray();
            return new(Scope, _query, Array.AsReadOnly(visible), Active?.Kind == "load", _loaded,
                Active?.Kind is "direct" or "direct-readback" ? _direct?.UserId : null,
                _readError, _openError, _direct is not null && Active is null);
        }
    }
    internal void Query(NativePersonalScope scope, string value)
    {
        lock (Gate) { if (!Matches(scope)) return; _query = value.Length <= 256 ? value : value[..256]; }
        Notify();
    }
    internal async Task LoadAsync(NativePersonalScope scope)
    {
        Operation? operation;
        lock (Gate) { operation = Begin(scope, "load"); if (operation is null) return; _readError = null; }
        Notify();
        try
        {
            var people = await commands.ListUsersAsync(scope, operation.Token);
            if (people.Select(person => person.Id).Distinct(StringComparer.Ordinal).Count() != people.Count)
                throw new InvalidDataException();
            lock (Gate) { if (Current(operation)) { _people = people.ToArray(); _loaded = true; } }
        }
        catch (OperationCanceledException) { lock (Gate) { if (Current(operation)) _readError = "Henting av personar vart avbroten. Prøv igjen."; } }
        catch { lock (Gate) { if (Current(operation)) _readError = "Personane kunne ikkje hentast. Prøv igjen."; } }
        finally { Finish(operation); }
    }
    internal async Task<string?> OpenDirectAsync(NativePersonalScope scope, string userId)
    {
        Operation? operation; DirectIntent intent;
        lock (Gate)
        {
            if (!Matches(scope) || _direct is not null || userId == scope.UserId || !_people.Any(person => person.Id == userId)) return null;
            operation = Begin(scope, "direct"); if (operation is null) return null;
            _direct = intent = new(userId, Guid.NewGuid().ToString("D")); _openError = null;
        }
        Notify();
        try
        {
            var receipt = await commands.OpenDirectAsync(scope, userId, intent.RequestId, operation.Token);
            lock (Gate)
            {
                if (!Current(operation)) return null;
                if (string.IsNullOrWhiteSpace(receipt.ChannelId)) throw new InvalidDataException();
                _direct = intent = intent with { ChannelId = receipt.ChannelId };
            }
            var channels = await commands.RefreshPersonalChannelsAsync(scope, operation.Token);
            lock (Gate)
            {
                if (!Current(operation)) return null;
                var match = channels.SingleOrDefault(channel => channel.Id == receipt.ChannelId && channel.IsDirect && channel.DirectUserId == userId);
                if (match is null) { _openError = "Samtalen er ikkje stadfesta. Kontroller henne før du prøver att."; return null; }
                _direct = null; return match.Id;
            }
        }
        catch (Exception error)
        {
            lock (Gate)
            {
                if (Current(operation))
                {
                    if (Rejected(error, intent.RequestId)) { _direct = null; _openError = "Samtalen kunne ikkje opnast. Prøv igjen."; }
                    else _openError = "Vi fekk ikkje stadfesta samtalen. Kontroller servertilstanden; førespurnaden blir ikkje send på nytt automatisk.";
                }
            }
            return null;
        }
        finally { Finish(operation); }
    }
    internal async Task<string?> RefreshUncertainDirectAsync(NativePersonalScope scope)
    {
        Operation? operation; DirectIntent intent;
        lock (Gate)
        {
            if (!Matches(scope) || _direct is not { } existing) return null;
            operation = Begin(scope, "direct-readback"); if (operation is null) return null;
            intent = existing; _openError = null;
        }
        Notify();
        try
        {
            var channels = await commands.RefreshPersonalChannelsAsync(scope, operation.Token);
            lock (Gate)
            {
                if (!Current(operation)) return null;
                var matching = channels.Where(channel => channel.IsDirect && channel.DirectUserId == intent.UserId &&
                    (intent.ChannelId is null || channel.Id == intent.ChannelId)).ToArray();
                if (matching.Length == 0)
                {
                    _direct = null;
                    _openError = "Fann inga stadfesta samtale. Kontrollen oppretta ingen samtale. Du kan velje ein person på nytt.";
                    return null;
                }
                if (matching.Length != 1)
                { _openError = "Fann fleire moglege samtalar. Ingen ny samtale er oppretta av kontrollen."; return null; }
                _direct = null; return matching[0].Id;
            }
        }
        catch (OperationCanceledException) { lock (Gate) { if (Current(operation)) _openError = "Kontrollen vart avbroten. Prøv kontrollen igjen."; } return null; }
        catch { lock (Gate) { if (Current(operation)) _openError = "Samtalen kunne ikkje kontrollerast. Prøv kontrollen igjen."; } return null; }
        finally { Finish(operation); }
    }
    internal void DismissUncertainDirect(NativePersonalScope scope)
    {
        lock (Gate)
        {
            if (!Matches(scope) || Active is not null || _direct is null) return;
            _direct = null;
            _openError = "Den førre samtalen er ikkje stadfesta. Ingen førespurnad er send på nytt.";
        }
        Notify();
    }
}
