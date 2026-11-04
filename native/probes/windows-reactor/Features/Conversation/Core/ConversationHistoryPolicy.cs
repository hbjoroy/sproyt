namespace Sproyt.WindowsProbe;

// The WinUI controller supplies measured visible rows after anchor restoration.
// This pure boundary never guesses visibility from latest_sequence or selection.
internal sealed record ConversationReadReport(string ChannelId, long ViewGeneration, IReadOnlyList<string> MessageIds);

internal static class ConversationHistoryPolicy
{
    internal static ConversationReadReport? VisibleRead(NativeChatSnapshot chat, bool activeWindow,
        bool visibleSurface, bool restoringAnchor, IEnumerable<string> measuredMessageIds)
    {
        if (!activeWindow || !visibleSurface || restoringAnchor || !chat.Connected || chat.Loading ||
            chat.SelectedChannel is not string channel || chat.History is not { } history || history.ChannelId != channel) return null;
        var roots = chat.Messages.Where(m => m.ChannelId == channel && m.ParentMessageId is null).Select(m => m.Id).ToHashSet(StringComparer.Ordinal);
        var ids = measuredMessageIds.Where(roots.Contains).Distinct(StringComparer.Ordinal).ToArray();
        return ids.Length == 0 ? null : new(channel, history.ViewGeneration, Array.AsReadOnly(ids));
    }
}
