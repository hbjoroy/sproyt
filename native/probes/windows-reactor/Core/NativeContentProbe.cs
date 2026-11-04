namespace Sproyt.WindowsProbe;

internal static class NativeContentProbe
{
    internal static async Task RunAsync()
    {
        const string marker = "[[media:00000000-0000-0000-0000-000000000001|image/png|Bilete%20%C3%B8.png]]";
        string source = "# Tittel\n\n**Sterkt** `" + marker + "`\n\n~~~~mermaid\ngraph LR\nA-->B\n" + marker + "\n~~~~\n" + marker;
        var parts = NativeMessageContent.ParseRichText(source, NativeRichText.LiteralRanges(source));
        Check(parts.Count(part => part.Kind == "media") == 1 && parts.Single(part => part.Kind == "media").Text == "Bilete ø.png", "A literal code marker became an attachment.");
        string literal = source[..source.LastIndexOf(marker, StringComparison.Ordinal)];
        Check(string.Concat(parts.Where(part => part.Kind == "text").Select(part => part.Text)) == literal, "Splitting wire content lost Markdown/diagram source bytes.");
        Check(NativeRichText.AttachmentSuffixVisible("Tekst\n" + marker, [marker]), "A plain attachment suffix was hidden.");
        foreach (string opening in new[] { "```csharp\n", "~~~~\n", "<script>\n", "<!--\n" })
            Check(!NativeRichText.AttachmentSuffixVisible(opening + marker, [marker]), "An open literal block swallowed an attachment without blocking send.");
        Check(NativeRichText.AttachmentSuffixVisible("```csharp\ncode\n```\n" + marker, [marker]), "Closing code did not release attachment sending.");
        string crowded = string.Join(' ', Enumerable.Repeat("[[x:a]]", 35)) + "\n" + marker;
        var crowdedParts = NativeMessageContent.ParseRichText(crowded, NativeRichText.LiteralRanges(crowded));
        Check(crowdedParts.Count < 64 && crowdedParts.Count(part => part.Kind == "media") == 1,
            "Earlier card markers hid the uploaded attachment suffix.");
        var scope = new NativeMessageActionScope("self", "1:family:1", "channel", 1);
        var message = new NativeMessage("message", "channel", "self", "Du", source, DateTimeOffset.UnixEpoch, 1);
        var actions = new NativeMessageActionController(); actions.Bind(scope);
        actions.Open(scope, message, NativeMessageActionMode.Edit, NativeRichText.LiteralRanges(source));
        Check(actions.HasAttachments && actions.Input.Contains("~~~~mermaid", StringComparison.Ordinal) && actions.Input.Contains(marker, StringComparison.Ordinal), "Editing removed literal code media markers.");
        actions.ChangeInput(scope, message.Id, NativeMessageActionMode.Edit, actions.Input + "\nEndra");
        string? changed = null;
        await actions.ConfirmAsync(scope, message, new(mutation => { changed = mutation.Value; return Task.CompletedTask; }, _ => Task.FromResult<bool?>(null)));
        Check(changed is not null && changed.Contains("~~~~mermaid", StringComparison.Ordinal) && changed.EndsWith(marker, StringComparison.Ordinal), "Editing did not retain the actual attachment.");
        string malformed = "[[media:------------------------------------|x|literal]]";
        actions.Open(scope, message with { Body = malformed }, NativeMessageActionMode.Edit, NativeRichText.LiteralRanges(malformed));
        Check(!actions.HasAttachments && actions.Input == malformed, "Malformed marker syntax was hidden in the editor.");
    }
    private static void Check(bool condition, string message) { if (!condition) throw new InvalidOperationException(message); }
}
