using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Windows.Foundation;

namespace Sproyt.WindowsProbe;

internal static class LiveTimelineUiProbe
{
    private static NativeTimelineViewport Timeline => LiveChatComponent.TimelineForProbe!;
    private static ScrollViewer Scroll => LiveChatComponent.ScrollForProbe!;

    internal static async Task RunAsync(FrameworkElement root, LiveUiFixture fixture)
    {
        bool previousActivation = Program.LiveWindowActive;
        try
        {
            Program.LiveWindowActive = false;
            fixture.PrepareHistory();
            await Until("initial narrow history", () => root.ActualWidth < 800 && Scroll.ViewportWidth > 0 &&
                Scroll.ScrollableHeight > Scroll.ViewportHeight * 2 && fixture.Chat.Messages.Count == 50 &&
                Rendered(root, fixture).Count > 0);
            Timeline.ObserveSoon();
            await Quiet("inactive initial history", () => fixture.ReadReports.Count == 0);

            // Use an off-tree target with an intentionally unhelpful absolute
            // offset. Only realization of the correct message can satisfy this.
            var candidates = fixture.Chat.Messages.Skip(5).SkipLast(10).ToArray();
            var target = candidates.FirstOrDefault(message => Find<FrameworkElement>(Scroll, "live-message-" + message.Id) is null)
                ?? throw new InvalidOperationException("Timeline fixture did not virtualize a candidate history row.");
            NativeScrollAnchor anchorA = new(target.Id, target.Sequence, -5, false, 0);
            Timeline.Restore(anchorA);
            Program.LiveWindowActive = true;
            Timeline.ObserveSoon();
            await AtAnchor(root, fixture, anchorA, "virtualized message");
            await Until("visible read receipt", () =>
            {
                var visible = Rendered(root, fixture);
                return visible.Count > 0 && fixture.ReadReports.Contains(visible.Max(row => row.Message.Sequence));
            });
            var firstView = Rendered(root, fixture);
            Require(firstView.All(row => row.Message.Sequence < 350) &&
                fixture.ReadReports.All(sequence => sequence <= firstView.Max(row => row.Message.Sequence)),
                "The first read receipt included offscreen messages.");

            // A sequence-only target must realize that row too; absolute zero
            // deliberately points elsewhere and cannot satisfy this check.
            var sequenceTarget = candidates.Last(message => message.Id != target.Id);
            NativeScrollAnchor sequenceAnchor = new(null, sequenceTarget.Sequence, -7, false, 0);
            Timeline.Restore(sequenceAnchor);
            await AtAnchor(root, fixture, sequenceAnchor, "sequence-only message");
            Timeline.Restore(anchorA);
            await AtAnchor(root, fixture, anchorA, "return from sequence-only message");

            // Exercise the real paging callback. The selected row must keep its
            // position even though there are fifty new rows ahead of it.
            int previousCalls = fixture.OlderCalls;
            await LiveChatComponent.LoadOlderForProbe!();
            await Until("older page", () => fixture.OlderCalls == previousCalls + 1 &&
                fixture.Chat.Messages.Any(message => message.Sequence < 301));
            await AtAnchor(root, fixture, anchorA, "prepended page");

            Program.LiveWindowActive = false;
            int receiptsBeforeScroll = fixture.ReadReports.Count;
            Timeline.UserIntentForProbe();
            await Until("inactive end of conversation", () =>
            {
                // ItemsRepeater refines its extent as variable-height rows are
                // realized. Follow the measured end until the final row appears.
                root.UpdateLayout();
                Scroll.ChangeView(null, Scroll.ScrollableHeight, null, true);
                return Rendered(root, fixture).Any(row => row.Message.Sequence == 350);
            });
            Timeline.ObserveSoon();
            await Quiet("deactivated scroller", () => fixture.ReadReports.Count == receiptsBeforeScroll);
            Program.LiveWindowActive = true;
            Timeline.Restore(anchorA);
            Timeline.ObserveSoon();
            await AtAnchor(root, fixture, anchorA, "return from inactive scroll");

            var composer = Find<TextBox>(root, "live-composer")!;
            composer.Text = "Utkast til Prat";
            await Until("first local draft", () => fixture.Workspace.Drafts["channel-a"].Body == "Utkast til Prat");
            await LiveChatComponent.NavigateForProbe!(new("channel-b"));
            await Until("second channel", () => fixture.Chat.SelectedChannel == "channel-b" &&
                Rendered(root, fixture).Count > 0 && !Timeline.Restoring);
            var targetB = fixture.Chat.Messages.Skip(fixture.Chat.Messages.Count / 2).First();
            NativeScrollAnchor anchorB = new(targetB.Id, targetB.Sequence, -9, false, 0);
            Timeline.Restore(anchorB);
            await AtAnchor(root, fixture, anchorB, "second channel anchor");
            Find<TextBox>(root, "live-composer")!.Text = "Utkast til Idear";
            await Until("second local draft", () => fixture.Workspace.Drafts["channel-b"].Body == "Utkast til Idear");

            await LiveChatComponent.BackForProbe!();
            await Until("back draft", () => fixture.Chat.SelectedChannel == "channel-a" &&
                Find<TextBox>(root, "live-composer")?.Text == "Utkast til Prat");
            await AtAnchor(root, fixture, anchorA, "back anchor");
            var predecessor = fixture.Chat.Messages.Last(message => message.Sequence < target.Sequence);
            fixture.ExpandMessage(predecessor.Id);
            await Until("earlier row expansion", () => fixture.Chat.Messages.Any(message =>
                message.Id == predecessor.Id && message.Body != predecessor.Body));
            await AtAnchor(root, fixture, anchorA, "earlier row height change");

            await LiveChatComponent.ForwardForProbe!();
            await Until("forward draft", () => fixture.Chat.SelectedChannel == "channel-b" &&
                Find<TextBox>(root, "live-composer")?.Text == "Utkast til Idear");
            await AtAnchor(root, fixture, anchorB, "forward anchor");
            // Exercise the same intent handler as wheel/key input, then drive
            // the native ScrollViewer. No physical input is injected here.
            Timeline.UserIntentForProbe();
            Scroll.ChangeView(null, 0, null, true);
            string topId = fixture.Chat.Messages[0].Id;
            string ScrollDetail()
            {
                var first = Rendered(root, fixture).FirstOrDefault();
                return $"offset={Scroll.VerticalOffset:F2}, firstId={first?.Message.Id}, Y={first?.Y:F2}, " +
                    $"restoring={Timeline.Restoring}, captureId={Timeline.Capture().MessageId}, expectedId={topId}, previousId={anchorB.MessageId}";
            }
            await Until("scroll away after restoration", () => Rendered(root, fixture).FirstOrDefault()?.Message.Id == topId &&
                Scroll.VerticalOffset <= 2, ScrollDetail);
            double topY = Rendered(root, fixture).First().Y;
            double topOffset = Scroll.VerticalOffset;
            void CheckPosition()
            {
                var first = Rendered(root, fixture).FirstOrDefault();
                if (first is null || first.Message.Id != topId || Math.Abs(first.Y - topY) > 2 ||
                    Math.Abs(Scroll.VerticalOffset - topOffset) > 2)
                    throw new InvalidOperationException($"Timeline scroll moved (expected Y={topY:F2}, offset={topOffset:F2}): " + ScrollDetail());
            }
            // Media layout may briefly re-apply the new top anchor. Measure
            // what a reader sees throughout that correction and until it settles.
            for (int sample = 0; sample < 12; sample++)
            {
                await Task.Delay(40);
                CheckPosition();
            }
            await Until("post-scroll layout settled", () => { CheckPosition(); return !Timeline.Restoring; }, ScrollDetail);
            Require(root.ActualWidth < 800 && Find<TextBox>(root, "live-composer")!.ActualWidth > 200 &&
                Scroll.ViewportWidth <= root.ActualWidth + 1, "Narrow history or composer exceeded the window.");
        }
        finally { Program.LiveWindowActive = previousActivation; }
    }

    private sealed record Visible(NativeMessage Message, double Y);

    // Independent geometry observation: Capture() alone would also return the
    // intended anchor during restoration when no matching row exists onscreen.
    private static IReadOnlyList<Visible> Rendered(FrameworkElement root, LiveUiFixture fixture)
    {
        root.UpdateLayout();
        var scroll = Scroll;
        var messages = fixture.Chat.Messages.ToDictionary(message => "live-message-" + message.Id);
        var rows = new List<Visible>();
        void Walk(DependencyObject parent)
        {
            if (parent is UIElement { Visibility: not Visibility.Visible }) return;
            if (parent is FrameworkElement element && messages.TryGetValue(AutomationProperties.GetAutomationId(element), out var message))
            {
                double y = element.TransformToVisual(scroll).TransformPoint(new Point()).Y;
                if (element.ActualHeight > 0 && y + element.ActualHeight > 1 && y < scroll.ViewportHeight - 1)
                    rows.Add(new(message, y));
                return;
            }
            for (int i = 0; i < VisualTreeHelper.GetChildrenCount(parent); i++) Walk(VisualTreeHelper.GetChild(parent, i));
        }
        Walk(scroll);
        return rows.OrderBy(row => row.Y).ToArray();
    }

    private static async Task AtAnchor(FrameworkElement root, LiveUiFixture fixture, NativeScrollAnchor anchor, string step)
    {
        bool Matches() => !Timeline.Restoring && !Timeline.AnchorUnavailable &&
            Rendered(root, fixture).FirstOrDefault() is { } first &&
            (anchor.MessageId is not null ? first.Message.Id == anchor.MessageId : first.Message.Sequence == anchor.Sequence) &&
            Math.Abs(first.Y - anchor.PixelOffset) <= 2;
        // Large rows and newly decoded media can require several layout passes.
        // Count only contiguous stable samples after ID and geometry agree.
        int stable = 0;
        await Until(step, () => (stable = Matches() ? stable + 1 : 0) >= 16);
    }

    private static T? Find<T>(DependencyObject parent, string id) where T : FrameworkElement
    {
        if (parent is T element && AutomationProperties.GetAutomationId(element) == id) return element;
        for (int i = 0; i < VisualTreeHelper.GetChildrenCount(parent); i++)
            if (Find<T>(VisualTreeHelper.GetChild(parent, i), id) is { } found) return found;
        return null;
    }

    private static async Task Until(string step, Func<bool> predicate, Func<string>? detail = null)
    {
        for (int i = 0; i < 320; i++)
        {
            if (predicate()) return;
            await Task.Delay(25);
        }
        throw new InvalidOperationException("Timeline fixture timeout: " + step + (detail is null ? "" : " (" + detail() + ")"));
    }

    private static async Task Quiet(string step, Func<bool> predicate)
    {
        for (int i = 0; i < 12; i++)
        {
            await Task.Delay(40);
            Require(predicate(), "Unstable timeline fixture: " + step);
        }
    }

    private static void Require(bool condition, string detail)
    {
        if (!condition) throw new InvalidOperationException(detail);
    }
}
