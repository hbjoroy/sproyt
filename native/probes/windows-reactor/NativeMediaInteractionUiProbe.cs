using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Automation.Provider;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;

namespace Sproyt.WindowsProbe;

// Actual mounted WinUI controls and private files, with synthetic shell/HTTP.
// No Explorer input, physical picker, user file or live service is involved.
internal static class NativeMediaInteractionUiProbe
{
    private const string FirstRoot = "conversation-root-one", SecondRoot = "conversation-root-two";

    internal static async Task RunAsync(FrameworkElement root, LiveUiFixture fixture)
    {
        var originalPicker = NativeMediaSave.PickDestinationForProbe;
        TaskCompletionSource<IReadOnlyList<NativeFileDropItem>>? resolving = null;
        TaskCompletionSource<string?>? choosing = null;
        try
        {
            fixture.PrepareContent("Utkast for filslipp");
            await Until("fresh root drop host", () => LiveChatComponent.RootDropForProbe is not null && Find<TextBox>(root, "live-composer") is { IsLoaded: true, Text: "" });
            var host = Find<FrameworkElement>(root, "live-file-drop")!;
            Require(host.AllowDrop && IsAncestor(host, Find<TextBox>(root, "live-composer")!), "The file-drop host does not cover the root editor.");
            Find<TextBox>(root, "live-composer")!.Text = "Kanal A";
            await Until("A text", () => fixture.Workspace.Drafts["channel-a"].ComposerText == "Kanal A");
            int messages = fixture.AcceptedMessageCount;
            var rootDrop = LiveChatComponent.RootDropForProbe!;
            resolving = new(TaskCreationOptions.RunContinuationsAsynchronously);
            var dropping = rootDrop.DropForProbeAsync(() => resolving.Task);
            await fixture.SelectAsync("channel-b");
            await Until("B editor", () => Find<TextBox>(root, "live-composer") is { IsLoaded: true, Text: "" });
            Find<TextBox>(root, "live-composer")!.Text = "Kanal B";
            await Until("B text", () => fixture.Workspace.Drafts["channel-b"].ComposerText == "Kanal B");
            resolving.SetResult([new("C:/synthetic-ui/utsikt.png")]);
            Require(await dropping, "A channel change cancelled a retained host's captured drop.");
            Require(fixture.LastAddedUploadTarget is { ChannelId: "channel-a", ParentMessageId: null } &&
                fixture.Workspace.Drafts["channel-a"].Attachments.Count == 1 && fixture.Workspace.Drafts["channel-b"].Attachments.Count == 0 &&
                fixture.Workspace.Drafts["channel-a"].ComposerText == "Kanal A" && fixture.Workspace.Drafts["channel-b"].ComposerText == "Kanal B" &&
                fixture.AcceptedMessageCount == messages, "A delayed drop changed text, sent a message or moved to the current channel.");
            Require(ReferenceEquals(rootDrop, LiveChatComponent.RootDropForProbe), "Channel navigation unnecessarily remounted the drop host.");
            Require(await rootDrop.DropForProbeAsync(() => Task.FromResult<IReadOnlyList<NativeFileDropItem>>([new("C:/synthetic-ui/kanal-b.pdf")])),
                "The retained host did not accept its next drop.");
            Require(fixture.LastAddedUploadTarget?.ChannelId == "channel-b", "A retained host kept its first render's channel.");
            fixture.MaximumFilesForNextAdmission = 0;
            Require(!await rootDrop.DropForProbeAsync(() => Task.FromResult<IReadOnlyList<NativeFileDropItem>>([new("C:/synthetic-ui/rejected.pdf")])),
                "Windows acknowledged a copied file that the staging owner rejected.");
            Require(!await rootDrop.DropForProbeAsync(() => Task.FromResult<IReadOnlyList<NativeFileDropItem>>([new("C:/synthetic-ui/folder", false)])),
                "A folder was admitted as a file.");
            await Until("scoped B error", () => Find<TextBlock>(root, "live-upload-error") is { IsLoaded: true, Visibility: Visibility.Visible });
            await fixture.SelectAsync("channel-a");
            await Until("A editor returned", () => Find<TextBox>(root, "live-composer")?.Text == "Kanal A");
            Require(Find<TextBlock>(root, "live-upload-error") is null, "A different channel displayed B's drop error.");

            await fixture.NavigateAsync(new("channel-a", ThreadId: FirstRoot));
            await Until("thread drop host", () => LiveChatComponent.ThreadDropForProbe is not null && Find<TextBox>(root, "live-thread-composer") is { IsLoaded: true });
            var threadHost = Find<FrameworkElement>(root, "live-thread-file-drop")!;
            Require(threadHost.AllowDrop && IsAncestor(threadHost, Find<TextBox>(root, "live-thread-composer")!), "The file-drop host does not cover the thread editor.");
            Find<TextBox>(root, "live-thread-composer")!.Text = "Eige trådutkast";
            string key = new ComposerTarget("channel-a", FirstRoot).Key;
            await Until("thread text", () => fixture.Workspace.Drafts[key].ComposerText == "Eige trådutkast");
            var threadDrop = LiveChatComponent.ThreadDropForProbe!;
            resolving = new(TaskCreationOptions.RunContinuationsAsynchronously);
            dropping = threadDrop.DropForProbeAsync(() => resolving.Task);
            await fixture.NavigateAsync(new("channel-a", ThreadId: SecondRoot));
            await Until("other thread", () => fixture.Capture().Threads?.Visit?.RootMessageId == SecondRoot && Find<TextBox>(root, "live-thread-composer")?.Text == "");
            resolving.SetResult([new("C:/synthetic-ui/trad.txt")]);
            Require(await dropping && fixture.LastAddedUploadTarget?.ParentMessageId == FirstRoot &&
                fixture.Workspace.Drafts[key].Attachments.Count == 1 && fixture.Workspace.Drafts[key].ComposerText == "Eige trådutkast" &&
                fixture.Workspace.Drafts[new ComposerTarget("channel-a", SecondRoot).Key].Attachments.Count == 0 && fixture.AcceptedMessageCount == messages,
                "A delayed thread drop escaped its original draft or sent a message.");

            // Production navigation may unmount the thread while awaiting the
            // next load. Such a retired host cancels unresolved shell work.
            resolving = new(TaskCreationOptions.RunContinuationsAsynchronously);
            dropping = LiveChatComponent.ThreadDropForProbe!.DropForProbeAsync(() => resolving.Task);
            await fixture.NavigateAsync(new("channel-a"));
            await Until("thread host retired", () => LiveChatComponent.ThreadDropForProbe is null);
            Require(!await dropping, "An unmounted thread kept accepting its pending drop.");
            resolving.TrySetResult([new("C:/synthetic-ui/retired.txt")]);
            Require(fixture.LastAddedUploadTarget?.ParentMessageId == FirstRoot && fixture.AcceptedMessageCount == messages,
                "A retired drop changed the draft or sent a message.");
            fixture.AddGallery();
            await OpenImage(root, fixture, 0);
            var viewer = NativeImageViewer.LastForProbe!;
            string original = viewer.OriginalPathForProbe!;
            Require(viewer.OriginalWidth == 2400 && viewer.OriginalHeight == 1200 && viewer.DecodedWidth == 2400 &&
                viewer.DecodedHeight == 1200 && !viewer.ReducedResolution && File.Exists(original), "Normal originals lost native detail or original geometry.");
            await Click(viewer.Root, "native-image-viewer-actual");
            Require(Near(viewer.Viewport.Scale, 1) && Find<TextBlock>(viewer.Root, "native-image-viewer-percent")!.Text.Contains("100"), "100% did not use oriented original dimensions.");
            viewer.Surface.Width = 320; viewer.Surface.Height = 180; root.UpdateLayout();
            await Until("resized original view", () => Near(viewer.Surface.ActualWidth, 320) && Near(viewer.Surface.ActualHeight, 180));
            Require(Near(viewer.Viewport.Scale, 1), "Resizing silently changed manually selected original scale.");
            await Click(viewer.Root, "native-image-viewer-fit");
            Require(Near(viewer.Viewport.Scale, Math.Min(320d / 2400, 180d / 1200)), "Whole-image fit did not use both viewport dimensions.");
            double fitted = viewer.Viewport.Scale;
            await Click(viewer.Root, "native-image-viewer-zoom-out");
            Require(viewer.Viewport.Scale < fitted, "Zoom out could not go below whole-image fit.");
            await Click(viewer.Root, "native-image-viewer-zoom-in");
            Require(Near(viewer.Viewport.Scale, fitted), "Zoom controls did not restore the expected scale.");

            string destination = Path.Combine(Program.LiveUiProbeDirectory!, "saved-original.png");
            NativeMediaSave.PickDestinationForProbe = (name, type, _) =>
            {
                Require(name.EndsWith(".png", StringComparison.Ordinal) && type == "image/png", "Save picker received an incorrect filename or MIME type.");
                return Task.FromResult<string?>(destination);
            };
            await Click(viewer.Root, "native-image-viewer-save");
            await Until("saved original", () => NativeMediaSave.LastSaveForProbe?.IsCompleted == true && File.Exists(destination));
            byte[] savedBytes = await File.ReadAllBytesAsync(destination), originalBytes = await File.ReadAllBytesAsync(original);
            Require(await NativeMediaSave.LastSaveForProbe! && savedBytes.SequenceEqual(originalBytes),
                "The explicit save action did not preserve original bytes.");
            await Click(viewer.Root, "native-image-viewer-close");
            await Until("closed original lease", () => viewer.IsDisposed && !File.Exists(original));
            await OpenImage(root, fixture, 1);
            viewer = NativeImageViewer.LastForProbe!; original = viewer.OriginalPathForProbe!;
            Require(viewer.OriginalWidth == 1200 && viewer.OriginalHeight == 2400, "Portrait original dimensions were replaced by decode or screen dimensions.");
            Require(Near(viewer.Viewport.Scale, Math.Min(1, viewer.Surface.ActualWidth / 1200)), "Portrait opening did not initially fit width.");
            await Click(viewer.Root, "native-image-viewer-fit");
            Require(Near(viewer.Viewport.Scale, Math.Min(1, Math.Min(viewer.Surface.ActualWidth / 1200, viewer.Surface.ActualHeight / 2400))), "Portrait fit cropped the original.");

            // An accepted save/drop cannot cross a real session retirement.
            choosing = new(TaskCreationOptions.RunContinuationsAsynchronously);
            string forbidden = Path.Combine(Program.LiveUiProbeDirectory!, "retired-original.png");
            NativeMediaSave.PickDestinationForProbe = (_, _, _) => choosing.Task;
            await Click(viewer.Root, "native-image-viewer-save");
            var pendingSave = NativeMediaSave.LastSaveForProbe!;
            await Until("save chooser pending", () => !pendingSave.IsCompleted);
            await fixture.InvalidateMediaForProbeAsync();
            choosing.TrySetResult(forbidden);
            try { await pendingSave; } catch (OperationCanceledException) { }
            await Until("retirement closes original", () => viewer.IsDisposed && !File.Exists(original) && NativeMediaPresenter.ActiveDialogForProbe is null);
            Require(!File.Exists(forbidden) && !Directory.EnumerateFiles(Program.LiveUiProbeDirectory!, ".sproyt-download-*").Any(),
                "A retired save published its destination or retained staging bytes.");
        }
        finally
        {
            resolving?.TrySetResult([]); choosing?.TrySetResult(null);
            NativeMediaSave.PickDestinationForProbe = originalPicker;
            NativeMediaPresenter.ActiveDialogForProbe?.Hide();
        }
    }

    private static async Task OpenImage(FrameworkElement root, LiveUiFixture fixture, int index)
    {
        string mediaId = NativeMediaViewFixture.GalleryId(index);
        var message = fixture.Chat.Messages.Single(message => message.Body.Contains(mediaId, StringComparison.Ordinal));
        LiveChatComponent.TimelineForProbe!.Restore(new(message.Id, message.Sequence, -5, false, 0));
        await Until("image row", () => Find<Button>(root, "native-media-open-" + mediaId) is { IsLoaded: true, IsEnabled: true });
        await Click(root, "native-media-open-" + mediaId);
        await Until("actual original dialog", () => NativeImageViewer.LastForProbe is { IsDisposed: false, Viewport.Ready: true } viewer &&
            viewer.Surface.ActualWidth > 0 && NativeMediaPresenter.ActiveDialogForProbe is not null);
    }
    private static bool Near(double left, double right) => Math.Abs(left - right) < .001;
    private static bool IsAncestor(DependencyObject parent, DependencyObject child)
    {
        for (DependencyObject? current = child; current is not null; current = VisualTreeHelper.GetParent(current)) if (ReferenceEquals(parent, current)) return true;
        return false;
    }
    private static async Task Click(DependencyObject root, string id)
    {
        await Until("button " + id, () => Find<Button>(root, id) is { IsLoaded: true, IsEnabled: true });
        if (new ButtonAutomationPeer(Find<Button>(root, id)!).GetPattern(PatternInterface.Invoke) is not IInvokeProvider invoke) throw new InvalidOperationException("Missing button Invoke: " + id);
        invoke.Invoke(); await Task.Delay(30);
    }
    private static T? Find<T>(DependencyObject root, string id) where T : FrameworkElement
    {
        if (root is UIElement { Visibility: not Visibility.Visible }) return null;
        if (root is T found && AutomationProperties.GetAutomationId(found) == id) return found;
        for (int index = 0; index < VisualTreeHelper.GetChildrenCount(root); index++) if (Find<T>(VisualTreeHelper.GetChild(root, index), id) is { } child) return child;
        return null;
    }
    private static async Task Until(string name, Func<bool> condition)
    {
        for (int attempt = 0; attempt < 400; attempt++) { if (condition()) return; await Task.Delay(25); }
        throw new InvalidOperationException("Media interaction fixture did not settle: " + name);
    }
    private static void Require(bool condition, string message) { if (!condition) throw new InvalidOperationException(message); }
}
