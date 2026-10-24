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
        window.Closed += (_, _) => File.WriteAllText(Path.Combine(folder, "ui-closed.txt"),
            Program.Workspace.State.Ready ? "FAIL: closed before shutdown" : "PASS: native Closed event after shutdown");
        try
        {
            await Until(() => root.ActualWidth > 500 && root.ActualHeight > 300);
            var composer = Find<TextBox>(root, "composer");
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
            await Until(() => Find<TextBox>(root, "composer").Text == "" && Program.Workspace.State.Outbox.Count == 1);
            Check(Program.Workspace.State.Outbox[0].Body == first, "The UI binding changed queued content.");

            window.SetSize(360, 720);
            await Until(() => root.ActualWidth < 800 && Find<Button>(root, "conversations").Visibility == Visibility.Visible);
            Check(Find<TextBox>(root, "composer").ActualWidth > 200, "Compact layout left no room to write.");
            Find<TextBox>(root, "composer").Text = "Dette skal vente etter avslutting";
            await Program.PrepareShutdownAsync();
            await using (var reopened = await NativeSession.OpenAsync(Path.Combine(Program.DataDirectory, "demo-v1.sqlite"), Program.LibraryPath))
            {
                var snapshot = await reopened.SnapshotAsync();
                Check(snapshot.Drafts.Any(value => value.Channel == "prat" && value.Body == "Dette skal vente etter avslutting"),
                    "Closing lost the latest editor text.");
                Check(snapshot.Drafts.Any(value => value.Channel == "idear" && value.Body == "Eit eige idéutkast"),
                    "Closing lost another channel's draft.");
                Check(snapshot.Outbox.Count == 1 && snapshot.Outbox[0].Body == first, "The accepted local message was not durable.");
            }
            File.WriteAllText(Path.Combine(folder, "ui-result.json"), JsonSerializer.Serialize(new
            {
                passed = true, architecture = System.Runtime.InteropServices.RuntimeInformation.ProcessArchitecture.ToString(),
                materializedTextBlocks = materialized, syntheticMessagesPerChannel = 1000,
                rasterizationScale = root.XamlRoot.RasterizationScale,
                compactWidthDip = root.ActualWidth,
                checks = new[] { "native WinUI tree", "virtualization", "editor identity and selection", "independent drafts",
                    "reading position", "light and dark semantic brushes", "UI enqueue binding", "compact layout", "flush and reopen" },
                notTested = new[] { "physical keyboard and IME", "touch", "screen reader", "visual screenshot approval", "production" },
            }, new JsonSerializerOptions { WriteIndented = true }));
        }
        catch (Exception exception)
        {
            Environment.ExitCode = 1;
            File.WriteAllText(Path.Combine(folder, "ui-failure.txt"), exception.ToString());
            try { await Program.Workspace.ShutdownAsync(); } catch { }
        }
        finally { Program.CloseAfterShutdown(window); }
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
