using System.Text;

namespace Sproyt.WindowsProbe;

// One presentation owner per personal surface. Scope contains the complete
// account/session and the host's visit, including A-B-A reopen generations.
// Network methods are called only by explicit host actions, never by Capture.
internal sealed class NativeInboxController : IDisposable
{
    internal const int MaximumMentions = 200, MaximumTasks = 2000, MaximumChannels = 2000, MaximumPendingActions = 32;
    private NativePersonalScope? _scope;
    private NativeInboxTab _tab;
    private CancellationTokenSource _lifetime = new();
    private readonly long[] _reads = new long[3];
    private readonly bool[] _loading = new bool[3], _ready = new bool[3], _capacity = new bool[3];
    private readonly string?[] _errors = new string?[3];
    private NativeChannel[] _channels = [];
    private NativeMention[] _mentions = [];
    private NativeAssignedTask[] _tasks = [];
    private HashSet<string> _availableChannels = new(StringComparer.Ordinal);
    private long _serial, _directoryRevision;
    private bool _disposed;
    private sealed record Source(string Channel, string Message, string? Parent, ulong Sequence);
    private Dictionary<string, Source> _known = new(StringComparer.Ordinal);
    private sealed record ActionState(long Serial, bool Busy, bool Uncertain, string? Error, bool? DesiredDone,
        NativeMention? Mention = null, NativeAssignedTask? Task = null)
    {
        internal NativeInboxActionState Projection => new(Busy, Uncertain, Error);
    }
    private readonly Dictionary<string, ActionState> _mentionActions = new(StringComparer.Ordinal), _taskActions = new(StringComparer.Ordinal);
    private sealed class CapacityException : IOException;
    internal event Action? Changed;

    internal void Bind(NativePersonalScope? scope, IReadOnlyList<NativeChannel>? channels = null,
        IReadOnlyList<NativeMessage>? knownMessages = null, bool directoryReady = true)
    {
        if (_disposed) return;
        bool changed = false;
        if (_scope != scope)
        {
            var retired = _lifetime; _lifetime = new();
            _scope = scope; ++_serial; ++_directoryRevision;
            _channels = []; _mentions = []; _tasks = []; _known.Clear(); _availableChannels.Clear();
            _mentionActions.Clear(); _taskActions.Clear();
            for (int index = 0; index < 3; index++) { ++_reads[index]; _loading[index] = _ready[index] = _capacity[index] = false; _errors[index] = null; }
            retired.Cancel(); retired.Dispose(); changed = true;
        }
        if (scope is null) { if (changed) Notify(); return; }
        if (channels is not null) changed |= ApplyChannels(channels, directoryReady);
        if (knownMessages is not null)
        {
            var next = new Dictionary<string, Source>(StringComparer.Ordinal); var ambiguous = new HashSet<string>(StringComparer.Ordinal);
            if (knownMessages.Count <= 5000)
                foreach (var message in knownMessages)
                {
                    if (!ValidMessage(message) || !ChannelAvailable(message.ChannelId) || ambiguous.Contains(message.Id)) continue;
                    var source = new Source(message.ChannelId, message.Id, message.ParentMessageId, message.Sequence);
                    if (next.TryGetValue(message.Id, out var previous) && previous != source) { next.Remove(message.Id); ambiguous.Add(message.Id); }
                    else next[message.Id] = source;
                }
            if (_known.Count != next.Count || next.Any(item => !_known.TryGetValue(item.Key, out var value) || value != item.Value))
            { _known = next; changed = true; }
        }
        if (changed) Notify();
    }
    private bool ApplyChannels(IReadOnlyList<NativeChannel> channels, bool ready)
    {
        int index = (int)NativeInboxTab.Unread;
        bool capacity = channels.Count > MaximumChannels;
        bool valid = !capacity && channels.All(channel => channel is not null && Identifier(channel.Id)) &&
            channels.Select(channel => channel.Id).Distinct(StringComparer.Ordinal).Count() == channels.Count;
        var next = valid ? channels.ToArray() : [];
        string? error = valid ? null : capacity ? "Samtalelista er større enn grensa i denne Windows-versjonen." : "Samtalelista kunne ikkje lesast.";
        if (_channels.SequenceEqual(next) && _ready[index] == (ready && valid) && _capacity[index] == capacity && (valid || _errors[index] == error)) return false;
        var permitted = ready && valid ? next.Select(channel => channel.Id).ToHashSet(StringComparer.Ordinal) : new HashSet<string>(StringComparer.Ordinal);
        bool lostAccess = _availableChannels.Any(id => !permitted.Contains(id));
        _channels = next; _availableChannels = permitted; _ready[index] = ready && valid; _capacity[index] = capacity; _errors[index] = error; ++_directoryRevision;
        _mentions = _mentions.Where(item => permitted.Contains(item.Message.ChannelId)).ToArray();
        foreach (var id in _mentionActions.Where(item => item.Value.Mention is { } mention && !permitted.Contains(mention.Message.ChannelId)).Select(item => item.Key).ToArray())
            _mentionActions.Remove(id);
        foreach (var id in _known.Where(item => !permitted.Contains(item.Value.Channel)).Select(item => item.Key).ToArray()) _known.Remove(id);
        if (lostAccess) InvalidateRead(NativeInboxTab.Mentions);
        return true;
    }
    internal void SelectTab(NativePersonalScope scope, NativeInboxTab tab)
    {
        if (!Current(scope) || !Enum.IsDefined(tab) || _tab == tab) return;
        _tab = tab; Notify();
    }
    internal NativeInboxSnapshot Capture()
    {
        var scope = _scope;
        if (scope is null) return new(null, _tab, false, false, null, false, [], [], []);
        var unread = _channels.Select(channel => new NativeInboxUnread(channel,
                channel.LatestSequence > channel.LastReadSequence ? channel.LatestSequence - channel.LastReadSequence : 0))
            .Where(item => item.Count > 0).OrderByDescending(item => item.Count).ThenBy(item => item.Channel.Id, StringComparer.Ordinal).ToArray();
        var mentions = _mentions.Where(mention => ChannelAvailable(mention.Message.ChannelId)).Select(mention => new NativeInboxMentionRow(mention, MentionState(mention.Message.Id),
            MentionDestination(scope, mention.Message.Id))).ToList();
        foreach (var (id, action) in _mentionActions)
            if ((action.Busy || action.Uncertain) && action.Mention is { } mention && ChannelAvailable(mention.Message.ChannelId) && !_mentions.Any(item => item.Message.Id == id))
                mentions.Add(new(mention, action.Projection, MentionDestination(scope, id), true));
        var tasks = _tasks.Select(task => new NativeInboxTaskRow(task, TaskState(task.Id), TaskDestination(scope, task.Id))).ToList();
        foreach (var (id, action) in _taskActions)
            if ((action.Busy || action.Uncertain) && action.Task is { } task && !_tasks.Any(item => item.Id == id))
                tasks.Add(new(task, action.Projection, TaskDestination(scope, id), true));
        int selected = (int)_tab;
        return new(scope, _tab, _loading[selected], _ready[selected], _errors[selected], _capacity[selected],
            Array.AsReadOnly(unread), Array.AsReadOnly(mentions.ToArray()), Array.AsReadOnly(tasks.ToArray()));
    }
    internal async Task LoadAsync(NativePersonalScope scope, NativeInboxTab tab, IPersonalCommands commands, CancellationToken cancellationToken = default)
    {
        if (!Current(scope) || !Enum.IsDefined(tab)) return;
        _tab = tab; int index = (int)tab; long read = ++_reads[index], directory = _directoryRevision;
        using var operation = CancellationTokenSource.CreateLinkedTokenSource(_lifetime.Token, cancellationToken);
        _loading[index] = true; _errors[index] = null; _capacity[index] = false; Notify();
        try
        {
            if (!ReadCurrent(scope, index, read)) return;
            if (tab == NativeInboxTab.Unread)
            {
                var channels = await commands.RefreshPersonalChannelsAsync(scope, operation.Token);
                if (ReadCurrent(scope, index, read) && directory == _directoryRevision) ApplyChannels(channels, true);
            }
            else if (tab == NativeInboxTab.Mentions)
            {
                var mentions = await commands.ListMentionsAsync(scope, operation.Token); ValidateMentions(mentions);
                if (ReadCurrent(scope, index, read))
                {
                    _mentions = mentions.Where(item => ChannelAvailable(item.Message.ChannelId)).ToArray();
                    PruneOldErrors(_mentionActions, _mentions.Select(item => item.Message.Id)); _ready[index] = true;
                }
            }
            else
            {
                var tasks = await commands.ListTasksAsync(scope, operation.Token); ValidateTasks(scope, tasks);
                if (ReadCurrent(scope, index, read))
                { _tasks = tasks.ToArray(); PruneOldErrors(_taskActions, _tasks.Select(item => item.Id)); _ready[index] = true; }
            }
        }
        catch (Exception failure)
        {
            if (ReadCurrent(scope, index, read))
            {
                _capacity[index] = failure is CapacityException;
                _errors[index] = failure is CapacityException ? "Lista er større enn grensa i denne Windows-versjonen." : "Lista kunne ikkje oppdaterast. Prøv igjen.";
            }
        }
        finally { if (ReadCurrent(scope, index, read)) { _loading[index] = false; Notify(); } }
    }
    internal async Task MarkMentionReadAsync(NativePersonalScope scope, string id, IPersonalCommands commands, CancellationToken cancellationToken = default)
    {
        var mention = FindMention(id);
        if (!Current(scope) || mention is null || mention.Read || !ChannelAvailable(mention.Message.ChannelId) || !CanStart(_mentionActions, id)) return;
        var action = new ActionState(++_serial, true, false, null, null, Mention: mention); _mentionActions[id] = action;
        using var operation = CancellationTokenSource.CreateLinkedTokenSource(_lifetime.Token, cancellationToken);
        Notify();
        try
        {
            if (!ActionCurrent(scope, id, action, _mentionActions)) return;
            string receipt = await commands.MarkMentionReadAsync(scope, id, Guid.NewGuid().ToString("N"), operation.Token);
            if (!ActionCurrent(scope, id, action, _mentionActions)) return;
            if (receipt != id) throw new IOException("Invalid mention receipt.");
            InvalidateRead(NativeInboxTab.Mentions);
            ReplaceMention(mention with { Read = true }); _mentionActions.Remove(id);
        }
        catch (Exception failure) { FailAction(scope, id, action, _mentionActions, failure); }
        finally { if (Current(scope)) Notify(); }
    }
    internal async Task SetTaskDoneAsync(NativePersonalScope scope, string id, bool done, IPersonalCommands commands, CancellationToken cancellationToken = default)
    {
        var task = FindTask(id);
        if (!Current(scope) || task is null || task.AssigneeId != scope.UserId || (task.Status == "done") == done || !CanStart(_taskActions, id)) return;
        var action = new ActionState(++_serial, true, false, null, done, Task: task); _taskActions[id] = action;
        using var operation = CancellationTokenSource.CreateLinkedTokenSource(_lifetime.Token, cancellationToken);
        Notify();
        try
        {
            if (!ActionCurrent(scope, id, action, _taskActions)) return;
            var receipt = await commands.SetTaskDoneAsync(scope, id, done, Guid.NewGuid().ToString("N"), operation.Token);
            if (!ActionCurrent(scope, id, action, _taskActions)) return;
            if (receipt.Id != id || receipt.AssigneeId != scope.UserId || receipt.SourceMessageId != task.SourceMessageId ||
                receipt.ChannelId != task.ChannelId || receipt.Status != (done ? "done" : "open")) throw new IOException("Invalid task receipt.");
            InvalidateRead(NativeInboxTab.Tasks);
            ReplaceTask(receipt); _taskActions.Remove(id);
        }
        catch (Exception failure) { FailAction(scope, id, action, _taskActions, failure); }
        finally { if (Current(scope)) Notify(); }
    }
    internal Task ReadBackMentionAsync(NativePersonalScope scope, string id, IPersonalCommands commands, CancellationToken cancellationToken = default) =>
        ReadBackAsync(scope, id, false, commands, cancellationToken);
    internal Task ReadBackTaskAsync(NativePersonalScope scope, string id, IPersonalCommands commands, CancellationToken cancellationToken = default) =>
        ReadBackAsync(scope, id, true, commands, cancellationToken);
    private async Task ReadBackAsync(NativePersonalScope scope, string id, bool task, IPersonalCommands commands, CancellationToken cancellationToken)
    {
        var actions = task ? _taskActions : _mentionActions;
        if (!Current(scope) || !actions.TryGetValue(id, out var previous) || !previous.Uncertain || previous.Busy) return;
        var action = previous with { Serial = ++_serial, Busy = true, Error = null }; actions[id] = action;
        using var operation = CancellationTokenSource.CreateLinkedTokenSource(_lifetime.Token, cancellationToken);
        Notify();
        try
        {
            if (!ActionCurrent(scope, id, action, actions)) return;
            if (task)
            {
                var rows = await commands.ListTasksAsync(scope, operation.Token); ValidateTasks(scope, rows);
                if (!ActionCurrent(scope, id, action, actions)) return;
                var found = rows.FirstOrDefault(item => item.Id == id);
                if (found is null) { actions[id] = action with { Busy = false, Error = "Oppgåva finst ikkje i den stadfesta lista. Ingen ny endring er send. Du kan lukke og opne innboksen for å hente lista på nytt." }; return; }
                if (found.ChannelId != action.Task!.ChannelId || found.SourceMessageId != action.Task.SourceMessageId) throw new IOException("Changed task identity.");
                InvalidateRead(NativeInboxTab.Tasks); ReplaceTask(found);
                actions[id] = action with { Busy = false, Uncertain = false, Error = (found.Status == "done") == action.DesiredDone ? null : "Serveren viser ei anna oppgåvestilling. Du kan velje ei ny endring sjølv." };
            }
            else
            {
                var rows = await commands.ListMentionsAsync(scope, operation.Token); ValidateMentions(rows);
                if (!ActionCurrent(scope, id, action, actions)) return;
                var found = rows.FirstOrDefault(item => item.Message.Id == id);
                if (found is null) { actions[id] = action with { Busy = false, Error = "Omtalen finst ikkje blant dei 200 nyaste tilgjengelege omtalane. Ingen ny lesehandling er send." }; return; }
                if (found.Message.ChannelId != action.Mention!.Message.ChannelId) throw new IOException("Changed mention identity.");
                InvalidateRead(NativeInboxTab.Mentions); ReplaceMention(found);
                actions[id] = action with { Busy = false, Uncertain = false, Error = found.Read ? null : "Serveren viser framleis omtalen som ulest. Du kan velje Marker lesen på nytt." };
            }
        }
        catch
        {
            if (ActionCurrent(scope, id, action, actions)) actions[id] = action with { Busy = false, Error = "Resultatet kunne ikkje kontrollerast. Ingen handling er gjenteken." };
        }
        finally { if (Current(scope)) Notify(); }
    }
    private void FailAction(NativePersonalScope scope, string id, ActionState action, Dictionary<string, ActionState> actions, Exception failure)
    {
        if (!ActionCurrent(scope, id, action, actions)) return;
        bool definite = failure is NativeCommandException { IsDefinitiveRejection: true };
        actions[id] = action with { Busy = false, Uncertain = !definite, Error = definite
            ? NativeClientWire.SafeMessage(failure.Message) : "Serveren har ikkje stadfesta handlinga. Kontroller resultatet før du prøver igjen." };
    }
    internal NativeInboxDestination? MentionDestination(NativePersonalScope scope, string id)
    {
        var mention = FindMention(id);
        return Current(scope) && mention is not null && ChannelAvailable(mention.Message.ChannelId) && ValidMessage(mention.Message)
            ? new(new(mention.Message.ChannelId, mention.Message.Id, mention.Message.Sequence, mention.Message.ParentMessageId), true) : null;
    }
    internal NativeInboxDestination? TaskDestination(NativePersonalScope scope, string id)
    {
        var task = FindTask(id);
        if (!Current(scope) || task is null || !ChannelAvailable(task.ChannelId)) return null;
        Source? source = _known.GetValueOrDefault(task.SourceMessageId);
        if (source is null)
        {
            var mention = _mentions.FirstOrDefault(item => item.Message.Id == task.SourceMessageId && item.Message.ChannelId == task.ChannelId);
            if (mention is not null) source = new(mention.Message.ChannelId, mention.Message.Id, mention.Message.ParentMessageId, mention.Message.Sequence);
        }
        return source is not null && source.Channel == task.ChannelId
            ? new(new(source.Channel, source.Message, source.Sequence, source.Parent), true)
            : new(new(task.ChannelId), false);
    }
    private bool ChannelAvailable(string id) => _ready[(int)NativeInboxTab.Unread] && _availableChannels.Contains(id);
    private NativeMention? FindMention(string id) => _mentions.FirstOrDefault(item => item.Message.Id == id) ??
        (_mentionActions.GetValueOrDefault(id) is { } action && (action.Busy || action.Uncertain) ? action.Mention : null);
    private NativeAssignedTask? FindTask(string id) => _tasks.FirstOrDefault(item => item.Id == id) ??
        (_taskActions.GetValueOrDefault(id) is { } action && (action.Busy || action.Uncertain) ? action.Task : null);
    private NativeInboxActionState MentionState(string id) => _mentionActions.GetValueOrDefault(id)?.Projection ?? new();
    private NativeInboxActionState TaskState(string id) => _taskActions.GetValueOrDefault(id)?.Projection ?? new();
    private void ReplaceMention(NativeMention mention) { _mentions = _mentions.Select(item => item.Message.Id == mention.Message.Id ? mention : item).ToArray(); }
    private void ReplaceTask(NativeAssignedTask task) { _tasks = _tasks.Select(item => item.Id == task.Id ? task : item).ToArray(); }
    private void InvalidateRead(NativeInboxTab tab) { ++_reads[(int)tab]; _loading[(int)tab] = false; }
    private bool CanStart(Dictionary<string, ActionState> actions, string id)
    {
        if (actions.TryGetValue(id, out var previous) && (previous.Busy || previous.Uncertain)) return false;
        if (actions.Values.Count(item => item.Busy || item.Uncertain) < MaximumPendingActions) return true;
        actions[id] = new(++_serial, false, false, "For mange handlingar ventar på svar. Vent eller kontroller dei uavklarte handlingane først.", null);
        Notify(); return false;
    }
    private static void PruneOldErrors(Dictionary<string, ActionState> actions, IEnumerable<string> ids)
    {
        var current = ids.ToHashSet(StringComparer.Ordinal);
        foreach (string id in actions.Where(item => !item.Value.Busy && !item.Value.Uncertain && !current.Contains(item.Key)).Select(item => item.Key).ToArray()) actions.Remove(id);
    }
    private bool Current(NativePersonalScope scope) => !_disposed && _scope == scope;
    private bool ReadCurrent(NativePersonalScope scope, int index, long read) => Current(scope) && _reads[index] == read;
    private bool ActionCurrent(NativePersonalScope scope, string id, ActionState action, Dictionary<string, ActionState> actions) =>
        Current(scope) && actions.TryGetValue(id, out var current) && current.Serial == action.Serial;
    private static bool Identifier(string? text) => text is { Length: > 0 and <= 128 } && !text.Any(char.IsControl);
    private static bool ValidMessage(NativeMessage? message) => message is not null && Identifier(message.Id) && Identifier(message.ChannelId) &&
        (message.ParentMessageId is null || Identifier(message.ParentMessageId) && message.ParentMessageId != message.Id);
    private static void ValidateMentions(IReadOnlyList<NativeMention> mentions)
    {
        if (mentions.Count > MaximumMentions) throw new CapacityException();
        if (mentions.Any(item => item is null || !ValidMessage(item.Message)) || mentions.Select(item => item.Message.Id).Distinct(StringComparer.Ordinal).Count() != mentions.Count)
            throw new IOException("Invalid mention list.");
    }
    private static void ValidateTasks(NativePersonalScope scope, IReadOnlyList<NativeAssignedTask> tasks)
    {
        if (tasks.Count > MaximumTasks) throw new CapacityException();
        if (tasks.Any(item => item is null || !Identifier(item.Id) || !Identifier(item.SourceMessageId) || !Identifier(item.ChannelId) ||
            item.AssigneeId != scope.UserId || item.Status is not ("open" or "done") || string.IsNullOrWhiteSpace(item.Title) || Encoding.UTF8.GetByteCount(item.Title) > 240) ||
            tasks.Select(item => item.Id).Distinct(StringComparer.Ordinal).Count() != tasks.Count) throw new IOException("Invalid task list.");
    }
    private void Notify() { try { Changed?.Invoke(); } catch { } }
    public void Dispose()
    {
        if (_disposed) return;
        Bind(null); _disposed = true; _lifetime.Cancel(); _lifetime.Dispose();
    }
}
