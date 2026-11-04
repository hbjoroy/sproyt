namespace Sproyt.WindowsProbe;

// Credential-free application navigation. Drafts remain in ChatWorkspace and
// transport/history remain in NativeChatClient; this owner never writes either.
internal sealed class NativeNavigationController
{
    private const int HistoryLimit = 64, AnchorLimit = 128, ChannelLimit = 512;
    private readonly object _gate = new();
    private readonly INativeNavigationPreferenceStore? _preferences;
    private readonly TimeProvider _clock;
    private readonly List<Entry> _history = [];
    private readonly Dictionary<NativeNavigationTarget, NativeScrollAnchor> _anchors = [];
    private NativeChannel[] _channels = [];
    private string[] _directOrder = [];
    private string? _user, _stamp, _family;
    private string _query = "";
    private NativeNavigationDirectoryScope _scope;
    private int _index;
    private bool _hasCurrent, _pending, _channelChanged, _interacting;
    private long _generation, _entryId;
    private string? _error, _directoryError, _preferenceError;

    internal NativeNavigationController(INativeNavigationPreferenceStore? preferences = null, TimeProvider? clock = null)
    { _preferences = preferences; _clock = clock ?? TimeProvider.System; }

    internal event Action? Changed;

    internal void BindScope(string? userId, string? sessionStamp)
    {
        bool changed;
        lock (_gate)
        {
            string? family = Family(sessionStamp);
            if (string.IsNullOrWhiteSpace(userId) || family is null) { userId = sessionStamp = family = null; }
            changed = userId != _user || sessionStamp != _stamp;
            if (!changed) return;
            bool sameFamily = userId is not null && userId == _user && family == _family;
            _user = userId; _stamp = sessionStamp; _family = family;
            _generation = checked(_generation + 1);
            _pending = false; _error = null;
            if (!sameFamily)
            {
                _history.Clear(); _anchors.Clear(); _channels = []; _directOrder = [];
                _index = 0; _hasCurrent = _interacting = false; _query = ""; _directoryError = _preferenceError = null;
                _scope = NativeNavigationDirectoryScope.Circles;
                if (_user is not null && _preferences is not null)
                {
                    try { _scope = ValidScope(_preferences.Read(_user).DirectoryScope); }
                    catch { _preferenceError = "Visingsvalet kunne ikkje lesast på denne eininga."; }
                }
            }
        }
        Notify();
    }

    internal bool UpdateChannels(string expectedUser, string expectedStamp, IReadOnlyList<NativeChannel> channels)
    {
        ArgumentNullException.ThrowIfNull(channels);
        lock (_gate)
        {
            if (!Owner(expectedUser, expectedStamp)) return false;
            // Invalid or ambiguous authoritative data fails closed. A search
            // projection is never accepted here in place of the full directory.
            bool valid = channels.Count <= ChannelLimit && channels.All(channel => channel is not null &&
                new NativeNavigationTarget(channel.Id).IsValid) &&
                channels.Select(channel => channel.Id).Distinct(StringComparer.Ordinal).Count() == channels.Count;
            if (valid && _directoryError is null && _channels.SequenceEqual(channels)) return true;
            if (!valid && _directoryError is not null && _channels.Length == 0) return true;
            _channels = valid ? channels.ToArray() : [];
            PruneAccess();
            _directoryError = valid ? null : "Samtalelista kunne ikkje lesast. Prøv å hente henne på nytt.";
        }
        Notify();
        return true;
    }

    internal NativeNavigationVisit? Select(NativeNavigationTarget target, NativeScrollAnchor? outgoingAnchor = null)
    {
        ArgumentNullException.ThrowIfNull(target);
        NativeNavigationVisit? visit;
        lock (_gate)
        {
            if (!Allowed(target)) return null;
            RememberCurrent(outgoingAnchor);
            if (CurrentEntry()?.Target == target)
            {
                if (!_pending && _error is null) return Visit();
                // Retrying/replacing an outstanding selection is a new async
                // operation even when the resource is unchanged. Fence the old
                // completion without adding a duplicate history entry.
                BeginVisit(target.ChannelId);
            }
            else
            {
                string? previousChannel = CurrentEntry()?.Target.ChannelId;
                int keep = _hasCurrent ? _index + 1 : _index;
                if (keep < _history.Count) _history.RemoveRange(keep, _history.Count - keep);
                var anchor = _anchors.GetValueOrDefault(target) ?? (target.MessageId is not null || target.Sequence is not null
                    ? new NativeScrollAnchor(target.MessageId, target.Sequence, AtEnd: false) : new NativeScrollAnchor());
                _history.Add(new(checked(++_entryId), target, anchor));
                if (_history.Count > HistoryLimit) _history.RemoveAt(0);
                _index = _history.Count - 1; _hasCurrent = true;
                BeginVisit(previousChannel);
            }
            visit = Visit();
        }
        Notify(); return visit;
    }

    internal NativeNavigationVisit? Back(NativeScrollAnchor? outgoingAnchor = null) => Move(-1, outgoingAnchor);
    internal NativeNavigationVisit? Forward(NativeScrollAnchor? outgoingAnchor = null) => Move(1, outgoingAnchor);

    // Reconnect/normal token rotation can invalidate an outstanding transport
    // callback without changing the desired resource or the user's directory
    // search/tab. Resume that resource under a fresh concrete visit fence.
    internal NativeNavigationVisit? RestartCurrent()
    {
        NativeNavigationVisit? visit;
        lock (_gate)
        {
            if (CurrentEntry() is not { } current || !Allowed(current.Target)) return null;
            _generation = checked(_generation + 1); _pending = true; _error = null;
            _channelChanged = false;
            visit = Visit();
        }
        Notify(); return visit;
    }

    private NativeNavigationVisit? Move(int direction, NativeScrollAnchor? outgoingAnchor)
    {
        NativeNavigationVisit? visit;
        lock (_gate)
        {
            int next = direction < 0 ? _index - 1 : _index + (_hasCurrent ? 1 : 0);
            if (_user is null || next < 0 || next >= _history.Count) return null;
            string? previousChannel = CurrentEntry()?.Target.ChannelId;
            RememberCurrent(outgoingAnchor);
            _index = next; _hasCurrent = true;
            BeginVisit(previousChannel);
            visit = Visit();
        }
        Notify(); return visit;
    }

    private void BeginVisit(string? previousChannel)
    {
        _generation = checked(_generation + 1); _pending = true; _error = null;
        var channel = _channels.Single(item => item.Id == _history[_index].Target.ChannelId);
        _channelChanged = channel.Id != previousChannel;
        var nextScope = channel.IsDirect ? NativeNavigationDirectoryScope.Direct : NativeNavigationDirectoryScope.Circles;
        if (_scope != nextScope) { _scope = nextScope; PersistScope(); }
    }

    internal bool SaveAnchor(NativeNavigationVisit visit, NativeScrollAnchor anchor)
    {
        ArgumentNullException.ThrowIfNull(anchor);
        lock (_gate)
        {
            if (!Current(visit) || !anchor.IsValid) return false;
            RememberCurrent(anchor); return true;
        }
    }

    internal bool IsCurrent(NativeNavigationVisit visit) { lock (_gate) return Current(visit); }

    // Linearize short operation registration with Select/Restart/BindScope.
    // The callback may only register/capture state: no await, network work or
    // cancellation callbacks while holding this navigation owner gate.
    internal bool AdmitCurrent(NativeNavigationVisit visit, Action admission)
    {
        ArgumentNullException.ThrowIfNull(admission);
        lock (_gate)
        {
            if (!Current(visit)) return false;
            admission(); return true;
        }
    }

    internal bool Complete(NativeNavigationVisit visit)
    {
        lock (_gate)
        {
            if (!Current(visit)) return false;
            if (!_pending && _error is null) return true;
            _pending = false; _error = null;
        }
        Notify(); return true;
    }

    internal bool Fail(NativeNavigationVisit visit, string message)
    {
        lock (_gate)
        {
            if (!Current(visit)) return false;
            _pending = false;
            _error = string.IsNullOrWhiteSpace(message) ? "Samtalen kunne ikkje opnast. Prøv igjen." :
                new string(message.Where(character => !char.IsControl(character)).Take(256).ToArray());
        }
        Notify(); return true;
    }

    internal void SetQuery(string query)
    {
        ArgumentNullException.ThrowIfNull(query);
        lock (_gate)
        {
            string next = _user is null ? "" : query[..Math.Min(query.Length, 256)];
            if (next == _query) return; _query = next;
        }
        Notify();
    }

    internal void SetDirectoryScope(NativeNavigationDirectoryScope scope)
    {
        lock (_gate)
        {
            scope = ValidScope(scope);
            if (_user is null || _scope == scope) return;
            _scope = scope; PersistScope();
        }
        Notify();
    }

    internal void SetInteraction(bool interacting)
    {
        lock (_gate) { if (_interacting == interacting) return; _interacting = interacting; }
        Notify();
    }

    internal NativeNavigationSnapshot Capture()
    {
        lock (_gate)
        {
            var ranked = RankDirects(_channels.Where(channel => channel.IsDirect), _clock.GetUtcNow());
            if (_interacting)
            {
                var remaining = ranked.ToDictionary(channel => channel.Id, StringComparer.Ordinal);
                ranked = _directOrder.Where(remaining.ContainsKey).Select(id => remaining[id])
                    .Concat(ranked.Where(channel => !_directOrder.Contains(channel.Id, StringComparer.Ordinal))).ToArray();
            }
            _directOrder = ranked.Select(channel => channel.Id).ToArray();
            string query = _query.Trim();
            bool Match(NativeChannel channel) => query.Length == 0 || channel.Name.Contains(query, StringComparison.CurrentCultureIgnoreCase) ||
                (channel.CircleName?.Contains(query, StringComparison.CurrentCultureIgnoreCase) ?? false) ||
                (channel.Slug?.Contains(query, StringComparison.CurrentCultureIgnoreCase) ?? false);
            var circleChannels = _channels.Where(channel => !channel.IsDirect && Match(channel)).ToArray();
            var directChannels = ranked.Where(Match).ToArray();
            var groups = new List<NativeNavigationGroup>();
            if (_scope == NativeNavigationDirectoryScope.Direct)
            {
                if (directChannels.Length > 0) groups.Add(new("direct", "Direkte", Array.AsReadOnly(directChannels)));
            }
            else
            {
                var shared = circleChannels.Where(channel => channel.CircleId is null).ToArray();
                if (shared.Length > 0) groups.Add(new("shared", "Felles", Array.AsReadOnly(shared)));
                foreach (var circle in circleChannels.Where(channel => channel.CircleId is not null).GroupBy(channel => channel.CircleId!, StringComparer.Ordinal)
                    .OrderBy(group => group.First().CircleName ?? "Vennekrets", StringComparer.CurrentCultureIgnoreCase).ThenBy(group => group.Key, StringComparer.Ordinal))
                    groups.Add(new("circle:" + circle.Key, circle.First().CircleName ?? "Vennekrets", Array.AsReadOnly(circle.ToArray())));
            }
            return new(_user, _query, _scope, Array.AsReadOnly(_channels.ToArray()), groups.AsReadOnly(), Visit(),
                _index > 0, _index + (_hasCurrent ? 1 : 0) < _history.Count, _pending, _error ?? _directoryError, _preferenceError,
                circleChannels.Length, directChannels.Length);
        }
    }

    private void PruneAccess()
    {
        var allowed = _channels.Select(channel => channel.Id).ToHashSet(StringComparer.Ordinal);
        var current = CurrentEntry();
        int prior = _history.Take(_index).Count(entry => allowed.Contains(entry.Target.ChannelId));
        _history.RemoveAll(entry => !allowed.Contains(entry.Target.ChannelId));
        foreach (var target in _anchors.Keys.Where(target => !allowed.Contains(target.ChannelId)).ToArray()) _anchors.Remove(target);
        _index = prior;
        _hasCurrent = current is not null && _history.Any(entry => entry.Id == current.Id);
        if (current is not null && !_hasCurrent)
        { _generation = checked(_generation + 1); _pending = false; }
    }

    private void RememberCurrent(NativeScrollAnchor? anchor)
    {
        if (anchor is null || !anchor.IsValid || CurrentEntry() is not { } entry) return;
        _history[_index] = entry with { Anchor = anchor };
        if (!_anchors.ContainsKey(entry.Target) && _anchors.Count >= AnchorLimit) _anchors.Remove(_anchors.Keys.First());
        _anchors[entry.Target] = anchor;
    }

    private bool Allowed(NativeNavigationTarget target) => _user is not null && target.IsValid && _channels.Any(channel => channel.Id == target.ChannelId);
    private bool Owner(string user, string stamp) => _user is not null && user == _user && stamp == _stamp;
    private bool Current(NativeNavigationVisit? visit) => visit is not null && Owner(visit.UserId, visit.SessionStamp) &&
        visit.Generation == _generation && CurrentEntry()?.Target == visit.Target && Allowed(visit.Target);
    private Entry? CurrentEntry() => _hasCurrent && _index >= 0 && _index < _history.Count ? _history[_index] : null;
    private NativeNavigationVisit? Visit() => CurrentEntry() is { } entry && _user is not null && _stamp is not null
        ? new(_user, _stamp, _generation, entry.Target, entry.Anchor, _channelChanged) : null;
    private static string? Family(string? stamp)
    {
        if (string.IsNullOrWhiteSpace(stamp)) return null;
        int last = stamp.LastIndexOf(':'); return last > 0 && last < stamp.Length - 1 ? stamp[..last] : null;
    }
    private static NativeNavigationDirectoryScope ValidScope(NativeNavigationDirectoryScope scope) =>
        scope == NativeNavigationDirectoryScope.Direct ? scope : NativeNavigationDirectoryScope.Circles;
    private void PersistScope()
    {
        if (_user is null || _preferences is null) return;
        try { _preferenceError = _preferences.Write(_user, new(_scope)) ? null : "Visingsvalet kunne ikkje lagrast på denne eininga."; }
        catch { _preferenceError = "Visingsvalet kunne ikkje lagrast på denne eininga."; }
    }
    private void Notify()
    {
        foreach (var callback in Changed?.GetInvocationList() ?? [])
            try { ((Action)callback)(); } catch (Exception exception) { System.Diagnostics.Debug.WriteLine(exception); }
    }
    private static NativeChannel[] RankDirects(IEnumerable<NativeChannel> channels, DateTimeOffset now)
    {
        var ranked = channels.Select(channel =>
        {
            var activity = channel.DirectActivity;
            DateTimeOffset last = activity?.LastMessageAt ?? DateTimeOffset.MinValue;
            if (last > now) last = now;
            double score = activity is not null && double.IsFinite(activity.Score)
                ? Math.Max(0, activity.Score) * Math.Pow(2, -Math.Max(0, (now - activity.CalculatedAt).TotalDays) / 30) : 0;
            return new { Channel = channel, Last = last, Score = score, Active = last >= now.AddHours(-36) };
        }).ToArray();
        Array.Sort(ranked, (left, right) =>
        {
            int order = right.Active.CompareTo(left.Active);
            if (order == 0) order = left.Active && right.Active ? right.Last.CompareTo(left.Last) : right.Score.CompareTo(left.Score);
            if (order == 0) order = right.Last.CompareTo(left.Last);
            return order == 0 ? StringComparer.Ordinal.Compare(left.Channel.Id, right.Channel.Id) : order;
        });
        return ranked.Select(item => item.Channel).ToArray();
    }
    private sealed record Entry(long Id, NativeNavigationTarget Target, NativeScrollAnchor Anchor);
}
