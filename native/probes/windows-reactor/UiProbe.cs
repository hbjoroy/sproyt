using Microsoft.UI.Reactor;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using System.Text.Json;

namespace Sproyt.WindowsProbe;

// An in-process integration test of our own WinUI tree and binding callbacks.
// It does not inject OS input and is not evidence of keyboard/IME/touch testing.
internal static class UiProbe
{
    private static int _started;
    private const string ClosingDraft = "Dette skal vente etter avslutting\r\nOgså den andre linja.";

    internal static void Attach(FrameworkElement root)
    {
        if (Program.UiProbeDirectory is null) return;
        RoutedEventHandler? loaded = null;
        loaded = (_, _) =>
        {
            root.Loaded -= loaded;
            if (Interlocked.Exchange(ref _started, 1) == 0) root.DispatcherQueue.TryEnqueue(() => Run(root));
        };
        root.Loaded += loaded;
        if (root.IsLoaded) loaded(root, new RoutedEventArgs());
    }

    private static async void Run(FrameworkElement root)
    {
        string folder = Program.UiProbeDirectory!;
        Directory.CreateDirectory(folder);
        var window = ReactorApp.PrimaryWindow!;
        window.Closed += (_, _) => File.WriteAllText(Path.Combine(folder, Program.UiResume ? "ui-resume-closed.txt" : "ui-closed.txt"),
            Program.Workspace.State.Ready ? "FAIL: closed before shutdown" : "PASS: native Closed event after shutdown");
        try
        {
            await Until(() => root.ActualWidth > 500 && root.ActualHeight > 300);
            var composer = Find<TextBox>(root, "composer");
            if (Program.UiResume)
            {
                // A new OS process must hydrate the actual editor, not merely
                // reopen SQLite successfully in the old process.
                await Until(() => SameText(composer.Text, ClosingDraft));
                Check(Program.Workspace.State.Outbox.Count == 2, "Restart lost a locally queued message.");
                PilotComponent.SelectForProbe!("idear");
                await Until(() => Find<TextBox>(root, "composer").Text == "Eit eige idéutkast");
                PilotComponent.SelectForProbe!("prat");
                await Until(() => SameText(Find<TextBox>(root, "composer").Text, ClosingDraft));
                await Program.PrepareShutdownAsync();
                File.WriteAllText(Path.Combine(folder, "ui-resume-result.json"), JsonSerializer.Serialize(new
                {
                    passed = true, architecture = System.Runtime.InteropServices.RuntimeInformation.ProcessArchitecture.ToString(),
                    checks = new[] { "new app process", "multiline editor hydration", "second channel draft", "queued message retained" },
                }));
                return;
            }
            int materialized = Descendants(root).OfType<TextBlock>().Count();
            Check(materialized is > 10 and < 250, "The synthetic history was not virtualized.");

            const string first = "Eit lokalt utkast 👋";
            composer.Text = first;
            composer.Select(4, 0);
            await Until(() => Saved("prat", first));
            Check(ReferenceEquals(composer, Find<TextBox>(root, "composer")) && composer.SelectionStart == 4,
                "Autosave replaced the editor or its selection.");

            PilotComponent.SelectForProbe!("idear");
            await Until(() => Find<TextBox>(root, "composer").Text == "");
            Find<TextBox>(root, "composer").Text = "Eit eige idéutkast";
            await Until(() => Saved("idear", "Eit eige idéutkast"));
            PilotComponent.SelectForProbe!("prat");
            await Until(() => Find<TextBox>(root, "composer").Text == first);

            var scroller = Find<ScrollViewer>(root, "timeline-prat");
            await Until(() => PilotComponent.ReadingPositionSettledForProbe!());
            scroller.ChangeView(null, 300, null, true);
            await Until(() => Math.Abs(scroller.VerticalOffset - 300) < 2);
            double readingOffset = scroller.VerticalOffset;
            PilotComponent.SelectForProbe!("idear");
            await Until(() => Find<TextBox>(root, "composer").Text == "Eit eige idéutkast");
            PilotComponent.SelectForProbe!("prat");
            await Until(() => Find<TextBox>(root, "composer").Text == first);
            await Until(() => PilotComponent.ReadingPositionSettledForProbe!() && scroller.Visibility == Visibility.Visible
                && Math.Abs(scroller.VerticalOffset - readingOffset) < 2);
            Check(ReferenceEquals(scroller, Find<ScrollViewer>(root, "timeline-prat")), "Channel switching replaced the scroller.");
            scroller.ChangeView(null, 650, null, true);
            await Until(() => Math.Abs(scroller.VerticalOffset - 650) < 2);
            PilotComponent.SelectForProbe!("idear");
            await Until(() => Find<TextBox>(root, "composer").Text == "Eit eige idéutkast");
            PilotComponent.SelectForProbe!("prat");
            await Until(() => PilotComponent.ReadingPositionSettledForProbe!() && Math.Abs(scroller.VerticalOffset - 650) < 2);
            PilotComponent.SelectForProbe!("idear");
            PilotComponent.SelectForProbe!("prat");
            await Until(() => PilotComponent.ReadingPositionSettledForProbe!() && Math.Abs(scroller.VerticalOffset - 650) < 2);

            PilotComponent.ThemeForProbe!(false);
            await Until(() => root.RequestedTheme == ElementTheme.Light);
            await Until(() => InkIs(root, 0x19, 0x1b, 0x18));
            PilotComponent.ThemeForProbe!(true);
            await Until(() => root.RequestedTheme == ElementTheme.Dark);
            await Until(() => InkIs(root, 0xeb, 0xec, 0xe1));

            await Until(() => Find<Button>(root, "queue-message").IsEnabled);
            await PilotComponent.SendForProbe!();
            bool repeatedSendRejected = false;
            try { await PilotComponent.SendForProbe!(); }
            catch (InvalidOperationException) { repeatedSendRejected = true; }
            Check(repeatedSendRejected && Program.Workspace.State.Outbox.Count == 1,
                "An immediate second send resurrected accepted editor text.");
            await Until(() => Find<TextBox>(root, "composer").Text == "" && Program.Workspace.State.Outbox.Count == 1);
            Check(Program.Workspace.State.Outbox[0].Body == first, "The UI binding changed queued content.");
            await Until(() => MessageVisible(scroller, first));

            const string beforeNavigation = "Ei sending rett før kanalbyte";
            Find<TextBox>(root, "composer").Text = beforeNavigation;
            await Until(() => Saved("prat", beforeNavigation));
            await PilotComponent.SendForProbe!();
            // No wait for the renderer to clear the old native TextBox here.
            PilotComponent.SelectForProbe!("idear");
            await Until(() => Find<TextBox>(root, "composer").Text == "Eit eige idéutkast");
            Check(Program.Workspace.State.Drafts["prat"].Body == "" && Program.Workspace.State.Outbox.Count == 2,
                "Immediate navigation resurrected accepted text.");
            PilotComponent.SelectForProbe!("prat");
            await Until(() => Find<TextBox>(root, "composer").Text == "");

            await Until(() => PilotComponent.ReadingPositionSettledForProbe!() && MessageVisible(scroller, beforeNavigation));
            int beforeResize = PilotComponent.ViewportAdjustmentsForProbe!();
            window.SetSize(1040, 700);
            await Task.Delay(40);
            window.SetSize(1000, 620);
            await Task.Delay(40);
            window.SetSize(960, 540);
            await Until(() => PilotComponent.ViewportAdjustmentsForProbe!() > beforeResize &&
                !PilotComponent.ViewportPendingForProbe!() && PilotComponent.ReadingPositionSettledForProbe!() && MessageVisible(scroller, beforeNavigation));
            await Task.Delay(250);
            Check(PilotComponent.ViewportAdjustmentsForProbe!() == beforeResize + 1, "Resize burst caused repeated tail corrections.");

            // The OS keyboard bridge uses this same debounced notification.
            int beforeKeyboard = PilotComponent.ViewportAdjustmentsForProbe!();
            PilotComponent.ViewportTransition!();
            PilotComponent.ViewportTransition!();
            await Until(() => PilotComponent.ViewportAdjustmentsForProbe!() == beforeKeyboard + 1 && PilotComponent.ReadingPositionSettledForProbe!());

            // Exercise actual native layout for an overlay keyboard. Merely
            // delivering a Showing notification does not make space for it.
            await Until(() => Program.Keyboard is not null);
            var keyboard = Program.Keyboard!;
            double clientHeight = root.ActualHeight;
            double unobstructedViewport = scroller.ViewportHeight;
            var occlusion = new Windows.Foundation.Rect(0, clientHeight - 160, root.ActualWidth, 160);
            var sameEditor = Find<TextBox>(root, "composer");
            keyboard.SetOcclusionForProbe(occlusion);
            await Until(() => Math.Abs(keyboard.Inset - 160) < 1 &&
                Math.Abs(scroller.ViewportHeight - (unobstructedViewport - 160)) < 1 &&
                PilotComponent.ReadingPositionSettledForProbe!() && !PilotComponent.ViewportPendingForProbe!() &&
                MessageVisible(scroller, beforeNavigation));
            Check(Math.Abs(root.ActualHeight - clientHeight) < 1 &&
                TopInRoot(root, sameEditor) + sameEditor.ActualHeight <= occlusion.Top &&
                ReferenceEquals(sameEditor, Find<TextBox>(root, "composer")),
                "Overlay keyboard hid/replaced the editor or changed the measurement root.");
            int afterOcclusion = PilotComponent.ViewportAdjustmentsForProbe!();
            keyboard.SetOcclusionForProbe(occlusion);
            await Task.Delay(250);
            Check(PilotComponent.ViewportAdjustmentsForProbe!() == afterOcclusion,
                "An unchanged keyboard rectangle caused repeated layout/scroll corrections.");

            // The same keyboard rectangle must not reserve space a second
            // time when Windows has already reduced the native client.
            window.SetSize(960, 460);
            await Until(() => root.ActualHeight < clientHeight - 40);
            double expectedInset = Math.Max(0, root.ActualHeight - occlusion.Top);
            await Until(() => Math.Abs(keyboard.Inset - expectedInset) < 1);
            keyboard.SetOcclusionForProbe(default(Windows.Foundation.Rect));
            await Until(() => keyboard.Inset == 0 && !PilotComponent.ViewportPendingForProbe!() &&
                PilotComponent.ReadingPositionSettledForProbe!() && MessageVisible(scroller, beforeNavigation));
            Check(KeyboardViewport.BottomOverlap(new(0, 0, 800, 400), new(0, 400, 800, 300)) == 0 &&
                KeyboardViewport.BottomOverlap(new(0, 0, 800, 600), new(0, 300, 400, 100)) == 0 &&
                KeyboardViewport.BottomOverlap(new(0, 0, 800, 600), new(900, 400, 800, 300)) == 0 &&
                Math.Abs(KeyboardViewport.BottomOverlap(new(0, 0, 800, 599.5), new(0, 420.25, 800, 300)) - 179.25) < .01,
                "Keyboard geometry confused a resized client, floating keyboard, other screen or fractional DIPs.");

            // Reading history must not be mistaken for following the tail.
            scroller.ChangeView(null, 650, null, true);
            await Until(() => Math.Abs(scroller.VerticalOffset - 650) < 2);
            await Task.Delay(220);
            var readingAnchor = VisibleAnchor(scroller);
            int whileReading = PilotComponent.ViewportAdjustmentsForProbe!();
            window.SetSize(960, 600);
            await Until(() => root.ActualHeight > 550);
            await Task.Delay(250);
            var sameAnchor = Descendants(scroller).OfType<FrameworkElement>()
                .FirstOrDefault(element => AutomationProperties.GetAutomationId(element) == readingAnchor.Id);
            Check(sameAnchor is not null && Math.Abs(TopIn(scroller, sameAnchor) - readingAnchor.Top) < 4 &&
                PilotComponent.ViewportAdjustmentsForProbe!() == whileReading,
                "Resizing moved the visible history message or forced a tail correction.");

            keyboard.SetOcclusionForProbe(new(0, root.ActualHeight - 140, root.ActualWidth, 140));
            await Until(() => Math.Abs(keyboard.Inset - 140) < 1);
            await Task.Delay(250);
            sameAnchor = Descendants(scroller).OfType<FrameworkElement>()
                .FirstOrDefault(element => AutomationProperties.GetAutomationId(element) == readingAnchor.Id);
            Check(sameAnchor is not null && Math.Abs(TopIn(scroller, sameAnchor) - readingAnchor.Top) < 4 &&
                PilotComponent.ViewportAdjustmentsForProbe!() == whileReading,
                "Opening the overlay keyboard dragged the history reader to the tail.");
            keyboard.SetOcclusionForProbe(default(Windows.Foundation.Rect));
            await Until(() => keyboard.Inset == 0);
            keyboard.SetOcclusionForProbe(null);

            window.SetSize(360, 720);
            await Until(() => root.ActualWidth < 800 && Find<Button>(root, "conversations").Visibility == Visibility.Visible &&
                Find<TextBox>(root, "composer").ActualWidth > 200);
            Check(Find<TextBox>(root, "composer").ActualWidth > 200, "Compact layout left no room to write.");
            Find<TextBox>(root, "composer").Text = ClosingDraft;
            await Program.PrepareShutdownAsync();
            await using (var reopened = await NativeSession.OpenAsync(Path.Combine(Program.DataDirectory, "demo-v1.sqlite"), Program.LibraryPath))
            {
                var snapshot = await reopened.SnapshotAsync();
                Check(snapshot.Drafts.Any(value => value.Channel == "prat" && SameText(value.Body, ClosingDraft)),
                    "Closing lost the latest editor text.");
                Check(snapshot.Drafts.Any(value => value.Channel == "idear" && value.Body == "Eit eige idéutkast"),
                    "Closing lost another channel's draft.");
                Check(snapshot.Outbox.Count == 2 && snapshot.Outbox[0].Body == first && snapshot.Outbox[1].Body == beforeNavigation,
                    "The accepted local messages were not durable.");
            }
            File.WriteAllText(Path.Combine(folder, "ui-result.json"), JsonSerializer.Serialize(new
            {
                passed = true, architecture = System.Runtime.InteropServices.RuntimeInformation.ProcessArchitecture.ToString(),
                materializedTextBlocks = materialized, syntheticMessagesPerChannel = 1000,
                rasterizationScale = root.XamlRoot.RasterizationScale,
                keyboardViewportAvailable = Program.KeyboardViewportAvailable,
                compactWidthDip = root.ActualWidth,
                checks = new[] { "native WinUI tree", "virtualization", "editor identity and selection", "independent drafts",
                    "reading position", "light and dark semantic brushes", "UI enqueue binding", "own message visible", "immediate send and navigation",
                    "debounced resize tail", "resize preserves history reader", "keyboard transition coalescing",
                    "overlay keyboard editor and tail visibility", "no double keyboard inset", "stable keyboard geometry",
                    "keyboard preserves history reader", "compact layout", "flush and reopen" },
                notTested = new[] { "physical keyboard and IME", "touch", "screen reader", "visual screenshot approval", "production" },
            }, new JsonSerializerOptions { WriteIndented = true }));
        }
        catch (Exception exception)
        {
            Environment.ExitCode = 1;
            File.WriteAllText(Path.Combine(folder, Program.UiResume ? "ui-resume-failure.txt" : "ui-failure.txt"), exception.ToString());
            try { await Program.Workspace.ShutdownAsync(); } catch { }
        }
        finally { Program.CloseAfterShutdown(window); }
    }

    private static bool SameText(string left, string right) => left.Replace("\r\n", "\n").Replace('\r', '\n')
        == right.Replace("\r\n", "\n").Replace('\r', '\n');

    private static double TopIn(ScrollViewer scroller, FrameworkElement element) =>
        element.TransformToVisual(scroller).TransformPoint(new Windows.Foundation.Point()).Y;

    private static double TopInRoot(FrameworkElement root, FrameworkElement element) =>
        element.TransformToVisual(root).TransformPoint(new Windows.Foundation.Point()).Y;

    private static (string Id, double Top) VisibleAnchor(ScrollViewer scroller)
    {
        var row = Descendants(scroller).OfType<FrameworkElement>()
            .Where(element => AutomationProperties.GetAutomationId(element).StartsWith("message-", StringComparison.Ordinal))
            .Select(element => (Element: element, Top: TopIn(scroller, element)))
            .Where(value => value.Top >= 0 && value.Top < scroller.ViewportHeight)
            .MinBy(value => value.Top);
        Check(row.Element is not null, "No visible history anchor was materialized.");
        return (AutomationProperties.GetAutomationId(row.Element!), row.Top);
    }

    private static bool MessageVisible(ScrollViewer scroller, string body)
    {
        var text = Descendants(scroller).OfType<TextBlock>().FirstOrDefault(value => value.Text == body);
        if (text is null || text.ActualHeight <= 0) return false;
        var bounds = text.TransformToVisual(scroller).TransformBounds(new Windows.Foundation.Rect(0, 0, text.ActualWidth, text.ActualHeight));
        return bounds.Top >= -1 && bounds.Bottom <= scroller.ActualHeight + 1;
    }

    private static bool Saved(string channel, string body)
    {
        var draft = Program.Workspace.State.Drafts[channel];
        return draft.Body == body && draft.Revision == draft.SavedRevision;
    }

    private static bool InkIs(DependencyObject root, byte r, byte g, byte b) => Descendants(root).OfType<TextBlock>()
        .Any(text => text.Text == "# Prat" && text.Foreground is SolidColorBrush brush && brush.Color.R == r && brush.Color.G == g && brush.Color.B == b);

    private static T Find<T>(DependencyObject root, string id) where T : FrameworkElement => Descendants(root).OfType<T>()
        .FirstOrDefault(element => AutomationProperties.GetAutomationId(element) == id) ?? throw new InvalidOperationException("Missing UI element: " + id);

    private static IEnumerable<DependencyObject> Descendants(DependencyObject root)
    {
        yield return root;
        for (int index = 0; index < VisualTreeHelper.GetChildrenCount(root); index++)
            foreach (var child in Descendants(VisualTreeHelper.GetChild(root, index))) yield return child;
    }

    private static async Task Until(Func<bool> ready)
    {
        var clock = System.Diagnostics.Stopwatch.StartNew();
        while (!ready())
        {
            if (clock.Elapsed > TimeSpan.FromSeconds(8)) throw new TimeoutException("WinUI binding did not settle.");
            await Task.Delay(15);
        }
        // Let the queued reconciler pass finish before the next native action.
        await Task.Delay(20);
    }

    private static void Check(bool condition, string message) { if (!condition) throw new InvalidOperationException(message); }
}
