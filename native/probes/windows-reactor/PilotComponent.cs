using Microsoft.UI.Input;
using Microsoft.UI.Reactor;
using Microsoft.UI.Reactor.Core;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using System.Text;
using Windows.System;
using Windows.UI.Core;
using static Microsoft.UI.Reactor.Factories;

namespace Sproyt.WindowsProbe;

internal sealed record TimelineMessage(string Id, string Author, string Time, string Body, bool Queued = false);

internal sealed class PilotComponent : Component
{
    internal static Action<string>? SelectForProbe;
    internal static Action<bool>? ThemeForProbe;
    internal static Func<Task>? SendForProbe;
    internal static FrameworkElement? RootForClose;
    internal static TextBox? EditorForClose;
    internal static Action? InvalidateForClose;
    private readonly Dictionary<string, (string Key, IReadOnlyList<TimelineMessage> Rows)> _timelines = [];
    private readonly Dictionary<string, ScrollViewer> _scrollers = [];
    private int _revision;

    public override Element Render()
    {
        var (_, invalidate) = UseState(0, threadSafe: true);
        InvalidateForClose = () => invalidate(Interlocked.Increment(ref _revision));
        var (channel, selectChannel) = UseState("prat");
        var (showList, setShowList) = UseState(false);
        var (dark, setDark) = UseState(Application.Current.RequestedTheme == ApplicationTheme.Dark);
        var wide = UseBreakpoint(800);
        UseEffect(() =>
        {
            void Changed() => invalidate(Interlocked.Increment(ref _revision));
            Program.Workspace.Changed += Changed;
            return () => Program.Workspace.Changed -= Changed;
        }, []);

        // Read the latest immutable coordinator snapshot on every render;
        // delayed invalidations cannot reinstall an older text snapshot.
        var state = Program.Workspace.State;
        var palette = dark ? SproytPalette.Dark : SproytPalette.Light;
        var ink = palette.Ink;
        var muted = palette.Muted;
        var paper = palette.Paper;
        var chrome = palette.Chrome;
        var surface = palette.Surface;
        var rule = palette.Rule;
        var selection = palette.Selection;
        var errorTone = palette.Error;
        bool conversationVisible = wide || !showList;
        string title = channel == "prat" ? "Prat" : "Idear";
        var draft = state.Drafts[channel];
        bool dirty = state.Drafts.Values.Any(value => value.Revision != value.SavedRevision);
        bool canSend = state.Ready && !Program.CloseAdmissionClosed && !state.Closing && !state.Terminal && !state.Sending && !state.NeedsRetry &&
            !string.IsNullOrWhiteSpace(draft.Body) && Encoding.UTF8.GetByteCount(draft.Body) <= 1024 && state.Outbox.Count < 8;

        void Select(string id) { if (!Program.CloseAdmissionClosed) { selectChannel(id); setShowList(false); } }
        Task EnqueueCurrent() => canSend ? Program.Workspace.SendAsync(channel) : Task.FromException(new InvalidOperationException("Sending is unavailable."));
        async void Send()
        {
            if (!canSend || Program.CloseAdmissionClosed) return;
            try { await EnqueueCurrent(); }
            catch { /* The coordinator publishes an inline, recoverable error. */ }
        }
        if (Program.UiProbeDirectory is not null)
        {
            SelectForProbe = Select;
            ThemeForProbe = setDark;
            SendForProbe = EnqueueCurrent;
        }
        async void Retry()
        {
            if (Program.CloseAdmissionClosed) return;
            try { await Program.Workspace.RetryAsync(); }
            catch { /* Keep the same request identity and visible draft. */ }
        }

        Element ChannelButton(string id, string name, string subtitle) => Button(
            VStack(4,
                TextBlock("# " + name).FontSize(16).SemiBold().Foreground(ink),
                TextBlock(subtitle).FontSize(12).Foreground(muted)
            ).HAlign(HorizontalAlignment.Left),
            () => Select(id))
            .AutomationId("channel-" + id).AutomationName("Opne kanalen " + name)
            .HorizontalContentAlignment(HorizontalAlignment.Stretch)
            .HAlign(HorizontalAlignment.Stretch).Padding(12).Margin(0, 4)
            .Background(channel == id ? selection : chrome)
            .BorderThickness(new Thickness(0)).CornerRadius(2);

        var sidebar = Grid([GridSize.Star()], [GridSize.Auto, GridSize.Star(), GridSize.Auto],
            VStack(8,
                TextBlock("Sprøyt").FontSize(28).Bold().Foreground(ink),
                TextBlock("WINDOWS-LABORATORIET").FontFamily("Cascadia Mono").FontSize(11).Foreground(muted)
            ).Padding(20, 24).Grid(row: 0),
            VStack(4,
                TextBlock("PRØVEKRETS").FontSize(12).Foreground(muted).Margin(12, 8),
                ChannelButton("prat", "Prat", "Ein roleg stad for samtalen"),
                ChannelButton("idear", "Idear", "Plass til det som kjem etterpå")
            ).Padding(8).Grid(row: 1),
            VStack(6,
                TextBlock("Berre lokale testdata").SemiBold().FontSize(13).Foreground(ink),
                TextBlock("Meldingane blir lagra på denne maskina. Prøva har inga nettilkopling.")
                    .TextWrapping().FontSize(12).Foreground(muted)
            ).Padding(20).Grid(row: 2)
        ).Background(chrome).WithKey("sidebar").IsVisible(wide || showList)
            .Grid(column: 0, columnSpan: wide ? 1 : 2);

        var context = Grid([GridSize.Auto, GridSize.Star(), GridSize.Auto], [GridSize.Auto],
            Button("←", () => { if (!Program.CloseAdmissionClosed) setShowList(true); }).FontSize(22).Size(44, 44)
                .AutomationId("conversations").AutomationName("Vis samtalar").ToolTip("Samtalar")
                .IsVisible(!wide).Margin(0, 0, 12, 0).Grid(column: 0),
            VStack(3,
                TextBlock("PRØVEKRETS").FontFamily("Cascadia Mono").FontSize(11).Foreground(muted),
                TextBlock("# " + title).FontSize(24).Bold().Foreground(ink)
            ).Grid(column: 1),
            HStack(12,
                TextBlock(state.Terminal ? "Lagringsdelen er lukka" : state.Closing ? "Lagrar og lukkar …" : dirty ? "Lagrar utkast …" : "● Lokalt lagra")
                    .AutomationId("draft-status").FontSize(12).Foreground(muted).VAlign(VerticalAlignment.Center),
                Button("◐", () => { if (!Program.CloseAdmissionClosed) setDark(!dark); }).Size(44, 44).FontSize(22)
                    .AutomationId("toggle-theme").AutomationName("Byt mellom lyst og mørkt tema").ToolTip("Byt tema")
            ).Grid(column: 2)
        ).Padding(20, 14);

        Element Row(TimelineMessage message) => VStack(6,
            Border(Empty()).Width(56).Height(1).Background(rule).HAlign(HorizontalAlignment.Left),
            HStack(10,
                TextBlock(message.Author).SemiBold().FontSize(14).Foreground(ink),
                TextBlock(message.Time).FontFamily("Cascadia Mono").FontSize(12).Foreground(muted),
                TextBlock(message.Queued ? "Lagra lokalt" : "").FontSize(12).Foreground(muted)
            ),
            TextBlock(message.Body).FontSize(16).Foreground(ink).TextWrapping().IsTextSelectionEnabled()
        ).Padding(20, 8).WithKey(message.Id);

        Element Timeline(string id) => LazyVStack<TimelineMessage>(TimelineRows(id, state.Outbox), item => item.Id, (item, _) => Row(item))
            .AutomationId("timeline-" + id).AutomationName("Meldingar i " + (id == "prat" ? "Prat" : "Idear"))
            .OnMount(element =>
            {
                if (element is not ScrollViewer scroller) return;
                _scrollers[id] = scroller;
                RoutedEventHandler? loaded = null;
                loaded = (_, _) =>
                {
                    scroller.Loaded -= loaded;
                    scroller.DispatcherQueue.TryEnqueue(Microsoft.UI.Dispatching.DispatcherQueuePriority.Low,
                        () => scroller.ChangeView(null, scroller.ScrollableHeight, null, true));
                };
                scroller.Loaded += loaded;
            })
            .OnUnmount(_ => _scrollers.Remove(id))
            .IsVisible(channel == id).WithKey("timeline-" + id);

        var history = Grid([GridSize.Star()], [GridSize.Star()], Timeline("prat"), Timeline("idear"));
        string error = ErrorText(state, channel);
        var notice = Grid([GridSize.Star(), GridSize.Auto], [GridSize.Auto],
            TextBlock(error).TextWrapping().FontSize(13).Foreground(state.ErrorCode is null ? muted : errorTone)
                .AutomationId("workspace-notice").LiveRegion().VAlign(VerticalAlignment.Center).Grid(column: 0),
            Button("Prøv same sending igjen", Retry).IsVisible(state.NeedsRetry).IsEnabled(!state.Sending && !state.Closing && !state.Terminal)
                .AutomationId("retry-send").Margin(12, 0, 0, 0).Grid(column: 1)
        ).Padding(20, 8).IsVisible(error.Length != 0);

        var input = TextBox(draft.Body, text =>
        {
            var latest = Program.Workspace.State;
            if (latest.Ready && !latest.Closing && !latest.Terminal && latest.Drafts[channel].Body != text)
                Program.Workspace.EditDraft(channel, text);
        }, placeholderText: "Skriv ei melding …")
            .AutomationId("composer").AutomationName("Melding i " + title)
            .AcceptsReturn().TextWrapping().IsSpellCheckEnabled(false)
            .IsReadOnly(Program.CloseAdmissionClosed || state.Closing || state.Terminal).IsEnabled(state.Ready || state.Terminal)
            .MinHeight(48).MaxHeight(132).FontSize(16).Padding(12).Foreground(ink).Background(surface)
            .BorderThickness(new Thickness(0)).CornerRadius(2)
            .OnKeyDown((_, args) =>
            {
                bool control = InputKeyboardSource.GetKeyStateForCurrentThread(VirtualKey.Control).HasFlag(CoreVirtualKeyStates.Down);
                if (args.Key == VirtualKey.Enter && control)
                {
                    args.Handled = true;
                    Send();
                }
            })
            .OnMount(element => EditorForClose = element as TextBox)
            .OnUnmount(element => { if (ReferenceEquals(EditorForClose, element)) EditorForClose = null; })
            .WithKey("composer-" + channel);
        var composer = Border(Grid([GridSize.Star(), GridSize.Auto], [GridSize.Auto, GridSize.Auto],
            input.Grid(column: 0),
            Button("↑", Send).FontSize(24).Size(48, 48).Margin(8, 0, 0, 0)
                .Foreground(palette.AccentInk).Background(palette.Accent).BorderThickness(new Thickness(0)).CornerRadius(2)
                .IsEnabled(canSend).AutomationId("queue-message").AutomationName("Lagre meldinga lokalt")
                .ToolTip("Lagre lokalt · Ctrl+Enter").Grid(column: 1),
            Grid([GridSize.Star(), GridSize.Auto], [GridSize.Auto],
                TextBlock("Ctrl+Enter lagrar · Enter gir ny linje").FontSize(11).Foreground(muted).Grid(column: 0),
                TextBlock($"{state.Outbox.Count}/8 i lokal prøvekø").FontSize(11).Foreground(muted).Grid(column: 1)
            ).Margin(2, 6, 0, 0).Grid(row: 1, columnSpan: 2)
        )).Padding(12).BorderThickness(new Thickness(0, 2, 0, 0)).BorderBrush(rule);

        var conversation = Grid([GridSize.Star()], [GridSize.Auto, GridSize.Star(), GridSize.Auto, GridSize.Auto],
            context.Grid(row: 0), history.Grid(row: 1), notice.Grid(row: 2), composer.Grid(row: 3)
        ).WithKey("conversation").IsVisible(conversationVisible).Grid(column: 1);

        return Grid([wide ? GridSize.Px(244) : GridSize.Px(0), GridSize.Star()], [GridSize.Star()], sidebar, conversation)
            .Background(paper).RequestedTheme(dark ? ElementTheme.Dark : ElementTheme.Light)
            .OnMount(element => { RootForClose = element; UiProbe.Attach(element); })
            .OnUnmount(element => { if (ReferenceEquals(RootForClose, element)) RootForClose = null; })
            .AutomationId("sproyt-root");
    }

    private IReadOnlyList<TimelineMessage> TimelineRows(string channel, IReadOnlyList<QueuedMessage> outbox)
    {
        var pending = outbox.Where(item => item.Channel == channel).OrderBy(item => item.RequestId, StringComparer.Ordinal).ToArray();
        string key = string.Join('|', pending.Select(item => item.RequestId));
        if (_timelines.TryGetValue(channel, out var cached) && cached.Key == key) return cached.Rows;
        string[] names = ["Kari", "Jon", "Ada"];
        string[] texts = channel == "prat"
            ? ["Her er det rom for sjølve samtalen.", "Eg likar at utkastet ventar når vi byter kanal.",
               "La oss halde det enkelt, og la innhaldet få plassen.", "For ein fin kveld. Kva har de lyst til å finne på i morgon?"]
            : ["Kva om vi tek ein tur ut og ser kvar dagen fører oss?", "Eg har ein idé vi kan prøve saman.",
               "Det viktigaste er at det er enkelt å kome i gang.", "Små steg, og god plass til dei gode samtalane."];
        var rows = Enumerable.Range(0, 1000).Select(index => new TimelineMessage($"{channel}-demo-{index:D4}",
            names[index % names.Length], $"{10 + index / 60 % 12:D2}:{index % 60:D2}", texts[index % texts.Length])).ToList();
        rows.Add(new(channel + "-welcome", "Sprøyt", "Prøve",
            "Dette er ei lokal Windows-prøve. Skriv litt, byt kanal og start appen på nytt: utkastet skal vente på deg. Historikken over er fiktiv."));
        rows.AddRange(pending.Select(item => new TimelineMessage(item.RequestId, "Du", "Lokal kø", item.Body, true)));
        _timelines[channel] = (key, rows);
        return rows;
    }

    private static string ErrorText(WorkspaceSnapshot state, string channel)
    {
        if (state.Terminal) return "Lagringsdelen er lukka. Du kan kopiere teksten og lukke prøva på nytt.";
        if (state.NeedsRetry) return "Vi fekk ikkje kvittering for førre sending. Prøv den same sendinga igjen; den nye teksten din er teken vare på.";
        if (state.ErrorCode == "text_too_large" || Encoding.UTF8.GetByteCount(state.Drafts[channel].Body) > 1024)
            return "Utkastet er for stort for denne prøva. Kort det ned før du lagrar eller avsluttar.";
        if (state.ErrorCode == "capacity" || state.Outbox.Count >= 8)
            return "Den lokale prøvekøen er full (8 meldingar). Utkastet ditt er teke vare på.";
        if (state.ErrorCode is not null) return "Vi fekk ikkje lagra no. Teksten står her; prøv å endre litt eller lukke på nytt.";
        return "";
    }
}
