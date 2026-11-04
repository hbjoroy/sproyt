namespace Sproyt.WindowsProbe;

// Owns presentation state only. Authentication, transport and durable drafts
// remain application-owned. Never advances read state optimistically.
internal sealed class NativeThreadFeatureController
{
    internal const int MaximumReplies = 2000, MaximumDirectoryRows = 2000;
    private const int MaximumAnchors = 64;
    private readonly object _gate = new();
    private readonly Dictionary<(string Channel, string Root), NativeScrollAnchor> _anchors = [];
    private string? _user, _stamp;
    private long _generation;
    private NativeThreadVisit? _visit;
    private NativeThreadSnapshot? _thread;
    private NativeThreadDirectoryItem[] _directory = [];
    private NativeScrollAnchor _anchor = new();
    private bool _loading, _atCapacity;
    private string? _error;

    internal event Action? Changed;

    internal void BindScope(string? userId, string? sessionStamp)
    {
        lock (_gate)
        {
            if (!Identifier(userId) || Family(sessionStamp) is null) userId = sessionStamp = null;
            if (_user == userId && _stamp == sessionStamp) return;
            bool sameFamily = _user == userId && userId is not null && Family(_stamp) == Family(sessionStamp);
            var previous = _visit;
            RememberAnchor();
            _user = userId; _stamp = sessionStamp; _generation = checked(_generation + 1);
            _thread = null; _directory = []; _error = null; _atCapacity = false;
            if (!sameFamily) { _anchors.Clear(); _visit = null; _anchor = new(); _loading = false; }
            else
            {
                _visit = previous is null ? null : new(userId!, sessionStamp!, _generation, previous.ChannelId, previous.RootMessageId);
                _loading = _visit is not null;
            }
        }
        Notify();
    }

    internal NativeThreadVisit? Open(string channelId, string rootMessageId) => Begin(channelId, rootMessageId);
    internal NativeThreadVisit? OpenDirectory(string channelId) => Begin(channelId, null);

    private NativeThreadVisit? Begin(string channelId, string? rootMessageId)
    {
        NativeThreadVisit visit;
        lock (_gate)
        {
            if (_user is null || _stamp is null || !Identifier(channelId) || (rootMessageId is not null && !Identifier(rootMessageId))) return null;
            RememberAnchor();
            visit = new(_user, _stamp, checked(++_generation), channelId, rootMessageId);
            _visit = visit; _loading = true; _error = null; _atCapacity = false;
            _thread = null; _directory = [];
            _anchor = rootMessageId is not null ? _anchors.GetValueOrDefault((channelId, rootMessageId)) ?? new() : new();
        }
        Notify(); return visit;
    }

    internal bool Close(NativeThreadVisit? expected = null)
    {
        lock (_gate)
        {
            if (expected is not null && !Current(expected)) return false;
            if (_visit is null) return true;
            RememberAnchor(); _visit = null; _generation = checked(_generation + 1);
            _thread = null; _directory = []; _loading = false; _error = null; _atCapacity = false; _anchor = new();
        }
        Notify(); return true;
    }

    internal bool IsCurrent(NativeThreadVisit visit) { lock (_gate) return Current(visit); }

    internal bool ApplyDirectory(NativeThreadVisit visit, IReadOnlyList<NativeThreadSummary> summaries,
        IReadOnlyList<NativeMessage>? rootPreviews = null)
    {
        ArgumentNullException.ThrowIfNull(summaries);
        lock (_gate)
        {
            if (!Current(visit) || !visit.IsDirectory) return false;
            bool valid = summaries.Count <= MaximumDirectoryRows && summaries.All(summary => summary is not null &&
                Identifier(summary.RootMessageId) && summary.UnreadCount <= summary.ReplyCount) &&
                summaries.Select(summary => summary.RootMessageId).Distinct(StringComparer.Ordinal).Count() == summaries.Count;
            if (!valid)
            {
                _directory = []; _loading = false; _atCapacity = summaries.Count > MaximumDirectoryRows;
                _error = _atCapacity ? "Samtalen har fleire trådar enn denne Windows-versjonen kan vise." : "Trådoversikta kunne ikkje lesast.";
            }
            else
            {
                var roots = (rootPreviews ?? []).Where(root => root.ChannelId == visit.ChannelId && root.ParentMessageId is null)
                    .GroupBy(root => root.Id, StringComparer.Ordinal).ToDictionary(group => group.Key, group => group.Last(), StringComparer.Ordinal);
                var directory = summaries.Where(summary => summary.ReplyCount > 0)
                    .OrderByDescending(summary => summary.UnreadCount > 0).ThenByDescending(summary => summary.LatestSequence)
                    .ThenBy(summary => summary.RootMessageId, StringComparer.Ordinal)
                    .Select(summary => new NativeThreadDirectoryItem(summary, roots.GetValueOrDefault(summary.RootMessageId))).ToArray();
                if (!_loading && _error is null && !_atCapacity && _directory.SequenceEqual(directory)) return true;
                _directory = directory;
                _loading = false; _atCapacity = false; _error = null;
            }
        }
        Notify(); return true;
    }

    internal bool ApplyThread(NativeThreadVisit visit, NativeThreadSnapshot thread)
    {
        ArgumentNullException.ThrowIfNull(thread);
        lock (_gate)
        {
            if (!Current(visit) || visit.IsDirectory || thread.ChannelId != visit.ChannelId || thread.RootMessageId != visit.RootMessageId ||
                (_thread is { } previous && (previous.ViewGeneration != thread.ViewGeneration || previous.Generation != thread.Generation))) return false;
            bool valid = thread.Replies.Count <= MaximumReplies &&
                (thread.Root is null || (thread.Root.Id == visit.RootMessageId && thread.Root.ChannelId == visit.ChannelId && thread.Root.ParentMessageId is null)) &&
                thread.Replies.All(reply => reply.ChannelId == visit.ChannelId && reply.ParentMessageId == visit.RootMessageId && reply.Id != visit.RootMessageId) &&
                thread.Replies.Select(reply => reply.Id).Distinct(StringComparer.Ordinal).Count() == thread.Replies.Count &&
                thread.Replies.Select(reply => reply.Sequence).Distinct().Count() == thread.Replies.Count;
            if (!valid)
            {
                _thread = null; _loading = false; _atCapacity = thread.Replies.Count > MaximumReplies;
                _error = _atCapacity ? "Tråden er større enn historikkgrensa i denne Windows-versjonen." : "Tråden kunne ikkje lesast.";
            }
            else
            {
                if (_thread is { } current && SameThread(current, thread) && _loading == thread.Loading &&
                    _error == thread.Error && _atCapacity == thread.AtCapacity) return true;
                // Immutable copies keep asynchronous UI observations tied to
                // the exact admitted projection rather than a mutable list.
                _thread = thread with { Replies = Array.AsReadOnly(thread.Replies.OrderBy(reply => reply.Sequence).ToArray()) };
                _loading = thread.Loading; _error = thread.Error; _atCapacity = thread.AtCapacity;
            }
        }
        Notify(); return true;
    }

    internal bool Fail(NativeThreadVisit visit, string message)
    {
        lock (_gate)
        {
            if (!Current(visit)) return false;
            _loading = false; _error = message;
        }
        Notify(); return true;
    }

    internal bool SaveAnchor(NativeThreadVisit visit, NativeScrollAnchor anchor)
    {
        ArgumentNullException.ThrowIfNull(anchor);
        lock (_gate)
        {
            if (!Current(visit) || visit.IsDirectory || !anchor.IsValid) return false;
            _anchor = anchor; RememberAnchor(); return true;
        }
    }

    internal NativeThreadReadReport? VisibleRead(NativeThreadVisit visit, long transportGeneration,
        bool activeWindow, bool visibleSurface, bool restoringAnchor, IEnumerable<string> measuredMessageIds)
    {
        ArgumentNullException.ThrowIfNull(measuredMessageIds);
        lock (_gate)
        {
            if (!Current(visit) || visit.IsDirectory || !activeWindow || !visibleSurface || restoringAnchor || _loading || _error is not null ||
                _thread is not { Loading: false, Error: null } thread || thread.Generation != transportGeneration) return null;
            var replies = thread.Replies.Select(reply => reply.Id).ToHashSet(StringComparer.Ordinal);
            var visible = measuredMessageIds.Where(replies.Contains).Distinct(StringComparer.Ordinal).ToArray();
            return visible.Length == 0 ? null : new(thread.ChannelId, thread.RootMessageId, thread.ViewGeneration, thread.Generation, Array.AsReadOnly(visible));
        }
    }

    internal NativeThreadFeatureSnapshot Capture()
    {
        lock (_gate) return new(_user, _visit, _loading, _error, _atCapacity, Array.AsReadOnly(_directory), _thread, _anchor);
    }

    private bool Current(NativeThreadVisit visit) => _visit == visit && _user == visit.UserId && _stamp == visit.SessionStamp;
    private static bool SameThread(NativeThreadSnapshot left, NativeThreadSnapshot right) =>
        left.ChannelId == right.ChannelId && left.RootMessageId == right.RootMessageId && left.ViewGeneration == right.ViewGeneration &&
        left.Generation == right.Generation && left.Root == right.Root && left.Replies.SequenceEqual(right.Replies.OrderBy(reply => reply.Sequence)) &&
        left.Loading == right.Loading && left.Error == right.Error && left.AtCapacity == right.AtCapacity && left.LastReadSequence == right.LastReadSequence;
    private void RememberAnchor()
    {
        if (_visit is not { RootMessageId: string root } visit) return;
        var key = (visit.ChannelId, root);
        if (!_anchors.ContainsKey(key) && _anchors.Count >= MaximumAnchors) _anchors.Remove(_anchors.Keys.First());
        _anchors[key] = _anchor;
    }
    private static bool Identifier(string? value) => value is { Length: > 0 and <= 128 } && !value.Any(char.IsControl);
    private static string? Family(string? stamp) => stamp?.LastIndexOf(':') is int colon && colon > 0 ? stamp[..colon] : null;
    private void Notify() { try { Changed?.Invoke(); } catch { } }
}
