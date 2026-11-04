using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Automation.Provider;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Documents;
using Microsoft.UI.Xaml.Media;

namespace Sproyt.WindowsProbe;

// Real native rendering and input bindings with synthetic upload/draft hooks.
// It never opens the physical picker, clipboard, server or a user file.
internal static class LiveContentUiProbe
{
    private const string RootId = "conversation-root-one";
    private static readonly string Literal = "[[media:" + NativeMediaViewFixture.Id + "|image/gif|literal.gif]]";
    private static readonly string Source = "# Innhaldsprøve\n\n**sterk** og *kursiv* med `kode`.\n\n" +
        "| Kolonne | Verdi |\n| --- | --- |\n| Første | Andre |\n\n```text\n" + Literal + "\n```\n\nSlutt på meldinga.\n";

    internal static async Task RunAsync(FrameworkElement root, LiveUiFixture fixture)
    {
        var originalPicker = NativeUploadShell.PickFilesForProbe;
        TaskCompletionSource<IReadOnlyList<string>>? picker = null;
        bool active = Program.LiveWindowActive;
        try
        {
            Program.LiveWindowActive = true; fixture.PrepareContent(Source);
            await Until("fresh content surface", () => fixture.Chat.Messages.Count == 3 &&
                Find<TextBox>(root, "live-composer") is { IsLoaded: true, Text: "" });
            await Reveal(root, RootId, 1);
            await Until("native rich table", () => Find<FrameworkElement>(Find<FrameworkElement>(root, "live-message-" + RootId), "native-rich-table") is { IsLoaded: true });
            var row = Find<FrameworkElement>(root, "live-message-" + RootId)!;
            var runs = Descendants<TextBlock>(row).SelectMany(block => Runs(block.Inlines)).ToArray();
            Require(runs.Any(run => run.Text == "sterk" && run.FontWeight.Weight >= 700), "Strong text did not use a native bold Run.");
            Require(runs.Any(run => run.Text.Contains(Literal, StringComparison.Ordinal) && run.FontFamily?.Source == "Cascadia Mono"), "A literal media marker in code lost its native code style.");
            Require(Find<Image>(row, "native-image-" + NativeMediaViewFixture.Id) is null, "A fenced literal marker created a live media preview.");
            var table = Find<ScrollViewer>(row, "native-rich-table")!;
            root.UpdateLayout();
            Require(table.HorizontalScrollMode == ScrollMode.Enabled && table.ActualWidth <= root.ActualWidth + 1,
                "A native table escaped the compact conversation width.");
            await Click(root, "native-message-raw-toggle-" + RootId);
            await Until("exact selectable raw source", () => Find<TextBlock>(root, "native-message-raw-" + RootId) is { IsTextSelectionEnabled: true } raw && raw.Text == Source,
                () =>
                {
                    var raw = Find<TextBlock>(root, "native-message-raw-" + RootId);
                    string actual = raw?.Text ?? "";
                    int mismatch = Enumerable.Range(0, Math.Min(actual.Length, Source.Length)).FirstOrDefault(index => actual[index] != Source[index], -1);
                    return $"rawPresent={raw is not null}, loaded={raw?.IsLoaded}, selectable={raw?.IsTextSelectionEnabled}, actualLength={actual.Length}, expectedLength={Source.Length}, mismatch={mismatch}, modelLength={fixture.Message(RootId)?.Body.Length}, tablePresent={Find<ScrollViewer>(Find<FrameworkElement>(root, "live-message-" + RootId), "native-rich-table") is not null}, restoring={LiveChatComponent.TimelineForProbe?.Restoring}";
                });
            await Click(root, "native-message-raw-toggle-" + RootId);
            await Until("formatted text returned", () => Find<ScrollViewer>(Find<FrameworkElement>(root, "live-message-" + RootId), "native-rich-table") is { IsLoaded: true });
            // A row refresh must update actions without replacing the editor
            // control and losing the caret on every character.
            await Click(root, "live-message-actions-" + RootId);
            await Click(root, "live-message-edit-" + RootId);
            await Until("content edit input", () => Find<TextBox>(root, "live-message-edit-input-" + RootId) is { IsLoaded: true });
            var editInput = Find<TextBox>(root, "live-message-edit-input-" + RootId)!;
            editInput.Focus(FocusState.Programmatic); editInput.Text = "A"; editInput.Select(1, 0);
            await Task.Delay(100);
            Require(ReferenceEquals(editInput, Find<TextBox>(root, "live-message-edit-input-" + RootId)) && editInput.FocusState != FocusState.Unfocused && editInput.SelectionStart == 1,
                "The first edit character replaced the focused native editor.");
            editInput.Text += "B"; editInput.Select(2, 0);
            await Task.Delay(100);
            Require(ReferenceEquals(editInput, Find<TextBox>(root, "live-message-edit-input-" + RootId)) && editInput.Text == "AB" && editInput.FocusState != FocusState.Unfocused && editInput.SelectionStart == 2,
                "A row update lost the second edit character or caret.");
            await Click(root, "live-message-cancel-" + RootId);
            Require(fixture.Message(RootId)?.Body == Source, "Cancelling the focus check changed the message.");
            await Reveal(root, "content-document", 7);
            await Until("generic document download action", () => Find<Button>(root, "native-attachment-download-" + LiveUiFixture.DocumentMediaId) is { IsLoaded: true, IsEnabled: true });
            Require(Find<Image>(Find<FrameworkElement>(root, "live-message-content-document"), "native-image-" + LiveUiFixture.DocumentMediaId) is null,
                "A document attachment was embedded as an image.");
            string downloaded;
            using (var document = await fixture.Media!.OriginalAsync(LiveUiFixture.DocumentMediaId, fixture.Media.Capture(), null, CancellationToken.None))
            {
                downloaded = document.Path;
                Require(document.ContentType == "application/pdf" && (await File.ReadAllTextAsync(downloaded)).StartsWith("%PDF-1.7\nsynthetic fixture document", StringComparison.Ordinal),
                    "Native document download did not stream the expected MIME and bytes.");
            }
            Require(!File.Exists(downloaded), "Document download left its private temporary file behind.");

            NativeUploadShell.PickFilesForProbe = () => Task.FromResult<IReadOnlyList<string>>(["S:/synthetic-ui/Blå%20 | [foto].gif"]);
            int accepted = fixture.AcceptedMessageCount;
            await Click(root, "live-attach-files");
            await Until("root attachment chip", () => fixture.Workspace.Drafts["channel-a"].Attachments.Count == 1 &&
                Find<FrameworkElement>(root, "live-draft-attachment-0") is { IsLoaded: true });
            Require(fixture.LastAddedUploadTarget is { ChannelId: "channel-a", ParentMessageId: null } && fixture.AcceptedMessageCount == accepted,
                "Upload changed target or sent chat automatically.");
            Require(Find<TextBox>(root, "live-composer")?.Text == "", "Attachment marker leaked into the text editor.");
            await Click(root, "live-draft-attachment-remove-0");
            await Until("root chip removal", () => fixture.Workspace.Drafts["channel-a"].Attachments.Count == 0);
            await Click(root, "live-attach-files");
            await Until("root attachment returns", () => fixture.Workspace.Drafts["channel-a"].Attachments.Count == 1);
            var composer = Find<TextBox>(root, "live-composer")!;
            composer.Text = "```\n";
            // WinUI normalizes multiline TextBox input to native line endings.
            // Compare the model with that actual value, while retaining the
            // open-fence syntax that must keep the attachment from being sent.
            string openCode = composer.Text;
            Require(openCode.StartsWith("```", StringComparison.Ordinal) && openCode.Length > 3 && openCode[3] is '\r' or '\n', "The native editor lost the open code fence.");
            await Until("open-code composer", () => fixture.Workspace.Drafts["channel-a"].ComposerText == openCode);
            await Until("hidden attachment send blocked", () => Find<Button>(root, "live-send") is { IsEnabled: false });
            Require(fixture.Workspace.Drafts["channel-a"].Attachments.Count == 1, "Syntax validation removed the pending attachment.");
            Find<TextBox>(root, "live-composer")!.Text = "";
            await Until("attachment-only send enabled", () => Find<Button>(root, "live-send") is { IsEnabled: true });
            string marker = fixture.Workspace.Drafts["channel-a"].Attachments.Single();
            await Click(root, "live-send");
            await Until("attachment-only root send", () => fixture.LastEnqueuedReply is { ParentMessageId: null } sent && sent.Body == marker && fixture.Workspace.Drafts["channel-a"].Attachments.Count == 0);

            fixture.HoldNextUpload = true;
            Find<TextBox>(root, "live-composer")!.Text = "Ventar på fila";
            await Until("text before active upload", () => fixture.Workspace.Drafts["channel-a"].ComposerText == "Ventar på fila");
            await Click(root, "live-attach-files");
            await Until("active upload", () => fixture.Capture().Uploads?.Jobs.SingleOrDefault()?.State == NativeUploadState.Uploading);
            string activeId = fixture.Capture().Uploads!.Jobs.Single().Id;
            await Until("active upload blocks send", () => Find<Button>(root, "live-send") is { IsEnabled: false });
            accepted = fixture.AcceptedMessageCount;
            await LiveChatComponent.SendForProbe!();
            Require(fixture.AcceptedMessageCount == accepted, "A retained send callback bypassed the active upload guard.");
            await Click(root, "live-upload-cancel-" + activeId);
            await Until("upload cancelled", () => fixture.Capture().Uploads!.Jobs.Single().State == NativeUploadState.Cancelled);
            await Click(root, "live-upload-remove-" + activeId);
            await Until("cancelled upload removed", () => fixture.Capture().Uploads!.Jobs.Count == 0 && Find<Button>(root, "live-send") is { IsEnabled: true });
            Find<TextBox>(root, "live-composer")!.Text = "";
            await Until("empty root before retry", () => fixture.Workspace.Drafts["channel-a"].ComposerText == "");
            fixture.FailNextUpload = true;
            NativeUploadShell.PickFilesForProbe = () => Task.FromResult<IReadOnlyList<string>>(["S:/synthetic-ui/rettleiing.pdf"]);
            await Click(root, "live-attach-files");
            await Until("failed upload", () => fixture.Capture().Uploads!.Jobs.SingleOrDefault()?.State == NativeUploadState.Failed);
            string failed = fixture.Capture().Uploads!.Jobs.Single().Id;
            await Click(root, "live-upload-retry-" + failed);
            await Until("upload retry attaches document", () => fixture.Capture().Uploads!.Jobs.Count == 0 && fixture.Workspace.Drafts["channel-a"].Attachments.Count == 1);
            await Click(root, "live-draft-attachment-remove-0");
            await Until("document chip removed", () => fixture.Workspace.Drafts["channel-a"].Attachments.Count == 0);

            await Reveal(root, RootId, 1); await Click(root, "live-open-thread-" + RootId);
            await Until("thread composer for upload", () => fixture.Capture().Threads?.Visit?.RootMessageId == RootId && Find<TextBox>(root, "live-thread-composer") is { IsLoaded: true, Text: "" });
            await Click(root, "native-message-raw-toggle-" + RootId);
            await Until("thread raw source", () => Find<TextBlock>(root, "native-message-raw-" + RootId) is { IsTextSelectionEnabled: true } raw && raw.Text == fixture.Message(RootId)!.Body);
            await Click(root, "native-message-raw-toggle-" + RootId);
            await Until("thread formatted source", () => Find<ScrollViewer>(Find<FrameworkElement>(root, "live-message-" + RootId), "native-rich-table") is { IsLoaded: true });
            string threadKey = new ComposerTarget("channel-a", RootId).Key;
            Find<TextBox>(root, "live-thread-composer")!.Text = "Utkast i tråden";
            await Until("thread text before picker", () => fixture.Workspace.Drafts[threadKey].ComposerText == "Utkast i tråden");
            picker = new(TaskCreationOptions.RunContinuationsAsynchronously);
            NativeUploadShell.PickFilesForProbe = () => picker.Task;
            await Click(root, "live-thread-attach-files");
            await Until("picker captured immutable thread target", () => fixture.LastCapturedUploadTarget?.ParentMessageId == RootId);
            await Click(root, "live-thread-close");
            await Until("channel while picker pending", () => fixture.Capture().Threads?.Visit is null && Find<TextBox>(root, "live-composer") is { IsLoaded: true, Text: "" });
            Find<TextBox>(root, "live-composer")!.Text = "Ny kanaltekst";
            await Until("new channel draft", () => fixture.Workspace.Drafts["channel-a"].ComposerText == "Ny kanaltekst");
            picker.TrySetResult(["S:/synthetic-ui/trådbilete.gif"]);
            await Until("offscreen target receives attachment", () => fixture.Workspace.Drafts[threadKey].Attachments.Count == 1);
            Require(fixture.LastAddedUploadTarget?.ParentMessageId == RootId && fixture.Workspace.Drafts["channel-a"].Attachments.Count == 0 &&
                fixture.Workspace.Drafts["channel-a"].ComposerText == "Ny kanaltekst", "A picker completion retargeted the now-active channel composer.");
            await Reveal(root, RootId, 1); await Click(root, "live-open-thread-" + RootId);
            await Until("retained thread chip", () => Find<FrameworkElement>(root, "live-thread-draft-attachment-0") is { IsLoaded: true } &&
                Find<TextBox>(root, "live-thread-composer")?.Text == "Utkast i tråden");
            await Click(root, "live-thread-draft-attachment-remove-0");
            await Until("thread chip removal keeps text", () => fixture.Workspace.Drafts[threadKey].Attachments.Count == 0 && fixture.Workspace.Drafts[threadKey].ComposerText == "Utkast i tråden");
            NativeUploadShell.PickFilesForProbe = () => Task.FromResult<IReadOnlyList<string>>(["S:/synthetic-ui/svar.gif"]);
            Find<TextBox>(root, "live-thread-composer")!.Text = "";
            await Until("empty thread text", () => fixture.Workspace.Drafts[threadKey].ComposerText == "");
            await Click(root, "live-thread-attach-files");
            await Until("thread attachment-only ready", () => fixture.Workspace.Drafts[threadKey].Attachments.Count == 1 && Find<Button>(root, "live-thread-send") is { IsEnabled: true });
            string threadMarker = fixture.Workspace.Drafts[threadKey].Attachments.Single();
            await Click(root, "live-thread-send");
            await Until("thread attachment-only send", () => fixture.LastEnqueuedReply is { ParentMessageId: RootId } sent && sent.Body == threadMarker && fixture.Workspace.Drafts[threadKey].Attachments.Count == 0);
            Require(fixture.Workspace.Drafts["channel-a"].ComposerText == "Ny kanaltekst", "Reply attachment send cleared another target's draft.");
            await Click(root, "live-thread-close");
            await Until("final channel draft preserved", () => Find<TextBox>(root, "live-composer")?.Text == "Ny kanaltekst");
        }
        finally
        {
            picker?.TrySetResult([]); NativeUploadShell.PickFilesForProbe = originalPicker; Program.LiveWindowActive = active;
        }
    }

    private static async Task Reveal(FrameworkElement root, string id, ulong sequence)
    {
        await Until("root viewport", () => LiveChatComponent.TimelineForProbe is not null && Find<ScrollViewer>(root, "live-history") is { IsLoaded: true });
        LiveChatComponent.TimelineForProbe!.Restore(new(id, sequence, -5, false, 0));
        await Until("message " + id, () => Find<FrameworkElement>(root, "live-message-" + id) is { IsLoaded: true });
    }
    private static async Task Click(FrameworkElement root, string id)
    {
        await Until("button " + id, () => Find<Button>(root, id) is { IsLoaded: true, IsEnabled: true });
        if (new ButtonAutomationPeer(Find<Button>(root, id)!).GetPattern(PatternInterface.Invoke) is not IInvokeProvider invoke)
            throw new InvalidOperationException("Native button does not expose Invoke: " + id);
        invoke.Invoke(); await Task.Delay(40);
    }
    private static async Task Until(string name, Func<bool> condition, Func<string>? detail = null)
    {
        for (int attempt = 0; attempt < 400; attempt++)
        { try { if (condition()) return; } catch (KeyNotFoundException) { } await Task.Delay(25); }
        throw new InvalidOperationException("Content UI fixture did not settle: " + name + (detail is null ? "" : "; " + detail()));
    }
    private static IEnumerable<T> Descendants<T>(DependencyObject root) where T : DependencyObject
    {
        if (root is UIElement { Visibility: not Visibility.Visible }) yield break;
        if (root is T found) yield return found;
        for (int index = 0; index < VisualTreeHelper.GetChildrenCount(root); index++)
            foreach (var child in Descendants<T>(VisualTreeHelper.GetChild(root, index))) yield return child;
    }
    private static T? Find<T>(DependencyObject? parent, string id) where T : FrameworkElement => parent is null ? null :
        Descendants<T>(parent).FirstOrDefault(element => AutomationProperties.GetAutomationId(element) == id);
    private static IEnumerable<Run> Runs(InlineCollection inlines)
    {
        foreach (var inline in inlines)
        {
            if (inline is Run run) yield return run;
            else if (inline is Span span) foreach (var child in Runs(span.Inlines)) yield return child;
        }
    }
    private static void Require(bool condition, string message) { if (!condition) throw new InvalidOperationException(message); }
}
