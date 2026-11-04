using System.Globalization;
using System.Text;
using System.Text.RegularExpressions;

namespace Sproyt.WindowsProbe;

internal sealed record NativeMessageActionScope(string UserId, string SessionStamp, string ChannelId,
    long ChannelVisit, string? ThreadRootId = null, long ThreadVisit = 0);
internal sealed record NativeReactionChip(string Emoji, uint Count, bool Mine, IReadOnlyList<string> Names);
internal enum NativeMessageMutationKind { Edit, Delete, ToggleReaction }
internal sealed record NativeMessageMutation(NativeMessageActionScope Scope, string MessageId,
    NativeMessageMutationKind Kind, string? Value = null, bool? ExpectedMine = null);
internal sealed record NativeMessageActionCommands(Func<NativeMessageMutation, Task> Execute,
    Func<NativeMessageMutation, Task<bool?>> ReadBack);
internal enum NativeMessageActionMode { Closed, Menu, Edit, Delete, Reactions }

// One UI-thread owner per surface. The root binds the current full credential and visit
// before painting. Every input, including callbacks retained by recycled rows, carries it.
internal sealed partial class NativeMessageActionController
{
    private NativeMessageActionScope? scope;
    private NativeMessageMutation? uncertain;
    private string[] attachments = [];
    private string? original;
    private string? initialInput;
    private long revision;
    private bool covered;
    internal void SetCovered(bool value) => covered = value;
    internal event Action? Changed;
    internal string? MessageId { get; private set; }
    internal NativeMessageActionMode Mode { get; private set; }
    internal string Input { get; private set; } = "";
    internal string? Error { get; private set; }
    internal bool Busy { get; private set; }
    internal bool NeedsReadBack => uncertain is not null;
    internal bool HasAttachments => attachments.Length != 0;
    internal bool CanSave => Input.Trim().Length != 0 || HasAttachments;
    internal static bool ValidEmoji(string text) => text.Length > 0 && Encoding.UTF8.GetByteCount(text) <= 32 &&
        StringInfo.ParseCombiningCharacters(text).Length == 1 && text.EnumerateRunes().Any(rune => rune.Value is
            0xa9 or 0xae or 0x203c or 0x2049 or 0x2122 or 0x2139 or 0x3030 or 0x303d or 0x3297 or 0x3299 or 0x20e3
            or >= 0x2190 and <= 0x21ff or >= 0x2300 and <= 0x23ff or >= 0x2460 and <= 0x24ff or >= 0x25a0 and <= 0x27bf
            or >= 0x1f000 and <= 0x1faff or >= 0x1fc00 and <= 0x1ffff);

    [GeneratedRegex(@"\[\[media:(?<id>[0-9a-fA-F-]{36})\|(?<type>[^|\]\r\n]+)\|[^\]\r\n]*\]\]", RegexOptions.CultureInvariant | RegexOptions.IgnoreCase)]
    private static partial Regex MediaTokens();
    private static bool ValidMediaToken(Match match) => Guid.TryParseExact(match.Groups["id"].Value, "D", out _) &&
        match.Groups["type"].Value.Length <= 128 && match.Groups["type"].Value.All(c => char.IsAsciiLetterOrDigit(c) || "/+.-".Contains(c));

    internal void Bind(NativeMessageActionScope? current)
    {
        if (scope == current) return;
        scope = current;
        Clear();
    }
    // Pass the complete materialized surface (not only the rows inside the viewport)
    // before rendering it. A remotely deleted target cannot keep the controls locked.
    internal void Observe(NativeMessageActionScope current, IReadOnlyList<NativeMessage> messages)
    {
        if (scope == current && MessageId is { } id && !messages.Any(m => m.Id == id && Available(current, m))) Clear();
    }
    private void Clear()
    {
        ++revision;
        MessageId = null; Mode = NativeMessageActionMode.Closed; Input = ""; Error = null;
        original = null; initialInput = null; attachments = []; uncertain = null; Busy = false;
    }
    internal bool Available(NativeMessageActionScope current, NativeMessage message) =>
        current == scope && message.ChannelId == current.ChannelId && message.DeletedAt is null &&
        !message.Id.StartsWith("pending:", StringComparison.Ordinal) &&
        (current.ThreadRootId is null ? message.ParentMessageId is null :
            message.Id == current.ThreadRootId || message.ParentMessageId == current.ThreadRootId);
    internal bool CanOpen(NativeMessageActionScope current, NativeMessage message) =>
        !covered && Available(current, message) && !Busy && !NeedsReadBack && (MessageId is null || MessageId == message.Id);
    internal void Open(NativeMessageActionScope current, NativeMessage message, NativeMessageActionMode mode,
        IReadOnlyList<(int Start, int End)>? literalRanges = null)
    {
        if (!CanOpen(current, message) || mode == NativeMessageActionMode.Closed ||
            (mode is NativeMessageActionMode.Edit or NativeMessageActionMode.Delete && message.AuthorId != current.UserId)) return;
        Clear();
        MessageId = message.Id; Mode = mode;
        if (mode == NativeMessageActionMode.Edit)
        {
            original = message.Body;
            var tokens = new List<string>();
            if (literalRanges is not null)
            {
                initialInput = Input = MediaTokens().Replace(original, match =>
                {
                    if (!ValidMediaToken(match) || literalRanges.Any(range => match.Index <= range.End && match.Index + match.Length - 1 >= range.Start)) return match.Value;
                    tokens.Add(match.Value); return "";
                }).Trim();
                attachments = tokens.ToArray(); Changed?.Invoke(); return;
            }
            var lines = original.Split('\n');
            bool fenced = false;
            for (int index = 0; index < lines.Length; index++)
            {
                string line = lines[index];
                // Keep literal wire syntax in code editable, as in NativeMessageContent.Parse.
                if (line.TrimStart().StartsWith("```", StringComparison.Ordinal)) { fenced = !fenced; continue; }
                if (fenced || line.Contains('`')) continue;
                lines[index] = MediaTokens().Replace(line, match => { if (!ValidMediaToken(match)) return match.Value; tokens.Add(match.Value); return ""; });
            }
            attachments = tokens.ToArray();
            initialInput = Input = string.Join("\n", lines).Trim();
        }
        Changed?.Invoke();
    }
    internal void Cancel(NativeMessageActionScope current, string id)
    {
        if (covered || scope != current || MessageId != id || Busy || NeedsReadBack) return;
        Clear(); Changed?.Invoke();
    }
    internal void ChangeInput(NativeMessageActionScope current, string id, NativeMessageActionMode mode, string input)
    {
        if (covered || scope != current || MessageId != id || Mode != mode || Busy || NeedsReadBack || Input == input) return;
        Input = input; Error = null; Changed?.Invoke();
    }
    internal async Task ConfirmAsync(NativeMessageActionScope current, NativeMessage message, NativeMessageActionCommands commands)
    {
        if (!CanOpen(current, message) || MessageId != message.Id || message.AuthorId != current.UserId) return;
        if (Mode == NativeMessageActionMode.Delete)
            await ExecuteAsync(new(current, message.Id, NativeMessageMutationKind.Delete), commands);
        else if (Mode == NativeMessageActionMode.Edit && CanSave)
        {
            if (message.Body != original)
            {
                Error = "Meldinga er endra. Kopier utkastet og opne redigering på nytt.";
                Changed?.Invoke(); return;
            }
            if (Input == initialInput) { Clear(); Changed?.Invoke(); return; }
            string body = string.Join("\n", new[] { Input.Trim() }.Concat(attachments).Where(s => s.Length != 0));
            await ExecuteAsync(new(current, message.Id, NativeMessageMutationKind.Edit, body), commands);
        }
    }
    internal async Task ToggleAsync(NativeMessageActionScope current, NativeMessage message, string emoji, bool mine,
        NativeMessageActionCommands commands)
    {
        emoji = emoji.Trim();
        if (!CanOpen(current, message) || Mode is not (NativeMessageActionMode.Closed or NativeMessageActionMode.Reactions)
            || !ValidEmoji(emoji)) return;
        MessageId = message.Id; Mode = NativeMessageActionMode.Reactions;
        await ExecuteAsync(new(current, message.Id, NativeMessageMutationKind.ToggleReaction, emoji, !mine), commands);
    }
    private async Task ExecuteAsync(NativeMessageMutation mutation, NativeMessageActionCommands commands)
    {
        long operation = ++revision;
        Busy = true; Error = null; Changed?.Invoke();
        try
        {
            if (operation != revision) return;
            await commands.Execute(mutation);
            if (operation == revision) Clear();
        }
        catch (Exception ex)
        {
            if (operation != revision) return;
            if (ex is NativeCommandException { IsDefinitiveRejection: true }) Error = ex.Message;
            else
            {
                uncertain = mutation;
                Error = "Svaret frå tenesta manglar. Kontroller om endringa vart lagra.";
            }
        }
        finally { if (operation == revision) Busy = false; Changed?.Invoke(); }
    }
    internal async Task CheckAsync(NativeMessageActionScope current, string id, NativeMessageActionCommands commands)
    {
        if (covered || current != scope || MessageId != id || Busy || uncertain is not { } mutation) return;
        long operation = ++revision;
        Busy = true; Changed?.Invoke();
        try
        {
            if (operation != revision) return;
            bool? result = await commands.ReadBack(mutation);
            if (operation != revision) return;
            if (result == true) Clear();
            else if (result == false)
            {
                uncertain = null;
                Error = "Tenesta viser ikkje den ønskte endringa. Du kan prøve igjen.";
            }
            else Error = "Endringa er framleis uviss. Kontroller på nytt.";
        }
        catch { if (operation == revision) Error = "Fekk ikkje kontrollert endringa."; }
        finally { if (operation == revision) Busy = false; Changed?.Invoke(); }
    }
}
