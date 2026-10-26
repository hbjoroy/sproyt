using System.Text.RegularExpressions;
using System.Text;

internal sealed record NativeConversationGroup(string Id, string Name, IReadOnlyList<NativeChannel> Channels);
internal sealed record NativeContentPart(string Kind, string Text, string? MediaId = null, string? ContentType = null);

// Sprøyt wire markers, not HTML or executable content. Shared by the Windows
// view and protocol fixtures; unknown cards retain a link to the original message.
internal static partial class NativeMessageContent
{
    [GeneratedRegex(@"\[\[([a-z][a-z0-9-]{0,39}):([^\]\r\n]{1,4096})\]\]", RegexOptions.CultureInvariant | RegexOptions.IgnoreCase)]
    private static partial Regex Marker();
    internal static IReadOnlyList<NativeConversationGroup> Groups(IReadOnlyList<NativeChannel> channels)
    {
        var result = new List<NativeConversationGroup>();
        void Add(string id, string name, IEnumerable<NativeChannel> items)
        { var list = items.ToArray(); if (list.Length > 0) result.Add(new(id, name, list)); }
        Add("shared", "Felles", channels.Where(c => c.CircleId is null && !c.IsDirect));
        foreach (var circle in channels.Where(c => c.CircleId is not null && !c.IsDirect)
            .GroupBy(c => c.CircleId!).OrderBy(g => g.First().CircleName ?? "Vennekrets", StringComparer.CurrentCultureIgnoreCase))
            Add("circle:" + circle.Key, circle.First().CircleName ?? "Vennekrets", circle);
        Add("direct", "Direkte", channels.Where(c => c.IsDirect));
        return result;
    }
    internal static IReadOnlyList<NativeContentPart> Parse(string body)
    {
        var parts = new List<NativeContentPart>();
        var pending = new StringBuilder();
        // Match the web treatment of code: wire-looking text in code stays text.
        bool fenced = false, diagram = false;
        foreach (string line in body[..Math.Min(body.Length, 65536)].Split('\n'))
        {
            if (!fenced && line.Trim().Equals("```mermaid", StringComparison.OrdinalIgnoreCase))
            { Add(new("diagram", "Diagram")); fenced = true; diagram = true; continue; }
            if (diagram)
            { if (line.TrimStart().StartsWith("```", StringComparison.Ordinal)) { fenced = false; diagram = false; } continue; }
            if (line.TrimStart().StartsWith("```", StringComparison.Ordinal)) fenced = !fenced;
            if (fenced || line.TrimStart().StartsWith("```", StringComparison.Ordinal) || line.Contains('`'))
            { Text(line + "\n"); continue; }
            int start = 0;
            foreach (Match match in Marker().Matches(line))
            {
                if (parts.Count >= 64) break;
                Text(line[start..match.Index]);
                string kind = match.Groups[1].Value.ToLowerInvariant(), value = match.Groups[2].Value;
                if (kind == "media")
                {
                    string[] fields = value.Split('|');
                    if (fields.Length == 3 && Guid.TryParseExact(fields[0], "D", out var id) &&
                        fields[1].Length <= 128 && fields[1].All(c => char.IsAsciiLetterOrDigit(c) || "/+.-".Contains(c)))
                    {
                        string name; try { name = Uri.UnescapeDataString(fields[2]); } catch { name = "Vedlegg"; }
                        name = new string(name.Where(c => !char.IsControl(c)).Take(200).ToArray());
                        Add(new("media", string.IsNullOrWhiteSpace(name) ? "Vedlegg" : name, id.ToString("D"), fields[1].ToLowerInvariant()));
                    }
                    else Text(match.Value);
                }
                else if ((kind == "invite" && (value.Length is < 32 or > 128 || !value.All(c => char.IsAsciiLetterOrDigit(c) || c is '-' or '_'))) ||
                    (kind is "process-task" or "work-item-task" or "work-item-status" && (!Guid.TryParseExact(value, "D", out _) || body != match.Value))) Text(match.Value);
                else Add(new(kind switch { "invite" => "invite", "process-task" or "work-item-task" => "task", "work-item-status" => "status", _ => "card" },
                    kind switch { "invite" => "Invitasjon", "process-task" or "work-item-task" => "Oppgåve", "work-item-status" => "Saksstatus", _ => "Innhald i Sprøyt" }));
                start = match.Index + match.Length;
            }
            Text(line[start..] + "\n");
        }
        Flush();
        if (body.Length > 65536) parts.Add(new("card", "Vis resten av meldinga"));
        return parts.Select(p => p.Kind == "text" ? p with { Text = p.Text.Trim('\n') } : p).Where(p => p.Text.Length > 0).ToArray();
        void Text(string text)
        {
            pending.Append(text);
        }
        void Flush() { if (pending.Length > 0) { parts.Add(new("text", pending.ToString())); pending.Clear(); } }
        void Add(NativeContentPart part) { Flush(); parts.Add(part); }
    }
}
