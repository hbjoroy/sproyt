using Microsoft.UI.Reactor;
using Microsoft.UI.Reactor.Core;
using Microsoft.UI.Xaml;
using System.Text;
using static Microsoft.UI.Reactor.Factories;

namespace Sproyt.WindowsProbe;

internal sealed record NativeInboxBindings(Action Close, Action<NativeInboxTab> SelectTab, Action Retry,
    Action<string> MarkMentionRead, Action<string> ReadBackMention, Action<string, bool> SetTaskDone,
    Action<string> ReadBackTask, Action<NativeNavigationTarget> Navigate);

// Credential-free presentation. The host keeps the conversation mounted while
// this destination is open, and owns its source/back navigation frame.
internal static class NativeInboxView
{
    internal static Element Render(NativeInboxController controller, SproytPalette palette, NativeInboxBindings bindings)
    {
        var state = controller.Capture();
        if (state.Scope is not { } scope) return Empty();
        bool Active() => controller.Capture().Scope == scope;
        void Navigate(Func<NativeInboxDestination?> destination)
        { if (Active() && destination() is { } target) bindings.Navigate(target.Target); }
        Element Tab(NativeInboxTab tab, string label) => Button(label, () => { if (Active()) bindings.SelectTab(tab); })
            .MinHeight(44).Padding(12, 8).AutomationName(label).AutomationId("live-inbox-tab-" + tab.ToString().ToLowerInvariant())
            .Background(state.Tab == tab ? palette.Accent : palette.Surface)
            .Foreground(state.Tab == tab ? palette.AccentInk : palette.Ink);
        Element Status(NativeInboxActionState action, string prefix, Action readBack) => VStack(4,
            TextBlock(action.Busy ? "Ventar på stadfesting …" : action.Error ?? "").TextWrapping()
                .Foreground(action.Error is null ? palette.Muted : palette.Error)
                .IsVisible(action.Busy || action.Error is not null).AutomationId(prefix + "-status"),
            Button("Kontroller resultatet", () => { if (Active()) readBack(); }).MinHeight(44)
                .IsEnabled(!action.Busy).IsVisible(action.NeedsReadBack).AutomationId(prefix + "-read-back"));
        Element Unread(NativeInboxUnread row) => Button(VStack(4,
                TextBlock(row.Channel.Name).SemiBold().Foreground(palette.Ink),
                TextBlock(row.Channel.CircleName ?? (row.Channel.IsDirect ? "Direktemelding" : "Felles")).FontSize(12).Foreground(palette.Muted),
                TextBlock($"{row.Count} uleste meldingar").Foreground(palette.Muted)),
            () =>
            {
                if (Active() && controller.Capture().Unread.Any(item => item.Channel.Id == row.Channel.Id))
                    bindings.Navigate(new(row.Channel.Id));
            }).HAlign(HorizontalAlignment.Stretch).HorizontalContentAlignment(HorizontalAlignment.Stretch)
            .Padding(16, 12).Margin(4, 2).MinHeight(56).Background(palette.Surface)
            .AutomationName($"Opne {row.Channel.Name}, {row.Count} uleste meldingar").AutomationId("live-inbox-unread-" + row.Channel.Id);
        Element Mention(NativeInboxMentionRow row)
        {
            var mention = row.Mention; var message = mention.Message; string prefix = "live-inbox-mention-" + message.Id;
            bool available = row.Destination is not null;
            return VStack(6,
                TextBlock(mention.ChannelName + " · " + message.Author + " · " + (mention.Read ? "Lesen" : "Ulest"))
                    .FontSize(12).TextWrapping().Foreground(palette.Muted),
                TextBlock(available ? message.DeletedAt is null ? Excerpt(message.Body) : "Sletta melding" : "Opphavssamtalen er ikkje tilgjengeleg.")
                    .TextWrapping().Foreground(palette.Ink).AutomationId(prefix + "-excerpt"),
                TextBlock("Sist kjende omtale med ei uavklart handling.").TextWrapping().Foreground(palette.Muted).IsVisible(row.Retained),
                HStack(8,
                    Button("Opne kjelda", () => Navigate(() => controller.MentionDestination(scope, message.Id))).MinHeight(44)
                        .IsEnabled(available).AutomationId(prefix + "-source"),
                    Button("Marker lesen", () => { if (Active()) bindings.MarkMentionRead(message.Id); }).MinHeight(44)
                        .IsVisible(!mention.Read).IsEnabled(available && !row.Action.Busy && !row.Action.NeedsReadBack).AutomationId(prefix + "-read")),
                Status(row.Action, prefix, () => bindings.ReadBackMention(message.Id))
            ).Padding(16, 12).Margin(4, 2).Background(palette.Surface).AutomationId(prefix);
        }
        Element TaskRow(NativeInboxTaskRow row)
        {
            var task = row.Task; bool done = task.Status == "done"; string prefix = "live-inbox-task-" + task.Id;
            return VStack(6,
                TextBlock(task.Title).SemiBold().TextWrapping().Foreground(palette.Ink).AutomationId(prefix + "-title"),
                TextBlock(task.ChannelName + " · " + (done ? "Ferdig" : "Open")).FontSize(12).TextWrapping().Foreground(palette.Muted),
                TextBlock(row.Destination is null ? "Opphavssamtalen er ikkje tilgjengeleg. Oppgåva er framleis di."
                    : !row.Destination.Exact ? "Kjeldemeldinga er ikkje lokalisert i denne visinga. Du kan opne samtalen." : "")
                    .TextWrapping().Foreground(palette.Muted).IsVisible(row.Destination?.Exact != true).AutomationId(prefix + "-source-status"),
                TextBlock("Sist kjende oppgåve med ei uavklart handling.").TextWrapping().Foreground(palette.Muted).IsVisible(row.Retained),
                HStack(8,
                    Button(done ? "Opne att" : "Ferdig", () => { if (Active()) bindings.SetTaskDone(task.Id, !done); }).MinHeight(44)
                        .AutomationName((done ? "Opne oppgåva att: " : "Fullfør oppgåva: ") + task.Title)
                        .IsEnabled(!row.Action.Busy && !row.Action.NeedsReadBack).AutomationId(prefix + "-toggle"),
                    Button(row.Destination?.Exact == true ? "Opne kjelda" : "Opne samtalen",
                        () => Navigate(() => controller.TaskDestination(scope, task.Id))).MinHeight(44)
                        .AutomationName((row.Destination?.Exact == true ? "Opne kjelda til oppgåva: " : "Opne samtalen for oppgåva: ") + task.Title)
                        .IsEnabled(row.Destination is not null).AutomationId(prefix + "-source")),
                Status(row.Action, prefix, () => bindings.ReadBackTask(task.Id))
            ).Padding(16, 12).Margin(4, 2).Background(palette.Surface).AutomationId(prefix);
        }
        Element content = state.Tab switch
        {
            NativeInboxTab.Unread => LazyVStack<NativeInboxUnread>(state.Unread, item => item.Channel.Id, (item, _) => Unread(item)),
            NativeInboxTab.Mentions => LazyVStack<NativeInboxMentionRow>(state.Mentions, item => item.Mention.Message.Id, (item, _) => Mention(item)),
            _ => LazyVStack<NativeInboxTaskRow>(state.Tasks, item => item.Task.Id, (item, _) => TaskRow(item))
        };
        int count = state.Tab switch { NativeInboxTab.Unread => state.Unread.Count, NativeInboxTab.Mentions => state.Mentions.Count, _ => state.Tasks.Count };
        string empty = state.Tab switch { NativeInboxTab.Unread => "Alt er lese.", NativeInboxTab.Mentions => "Ingen omtalar i den tilgjengelege lista.", _ => "Du har ingen personlege oppgåver." };
        return Grid([GridSize.Star()], [GridSize.Auto, GridSize.Star()],
            VStack(8,
                Grid([GridSize.Star(), GridSize.Auto], [GridSize.Auto],
                    TextBlock("Innboks").FontSize(21).SemiBold().Foreground(palette.Ink).VAlign(VerticalAlignment.Center).Grid(column: 0),
                    HStack(6,
                        Button("Oppdater", () => { if (Active()) bindings.Retry(); }).MinHeight(44).IsEnabled(!state.Loading)
                            .AutomationName("Oppdater innboksen").AutomationId("live-inbox-refresh"),
                        Button("Tilbake", () => { if (Active()) bindings.Close(); }).MinHeight(44)
                            .AutomationName("Tilbake til samtalen").AutomationId("live-inbox-close")).Grid(column: 1)),
                HStack(4, Tab(NativeInboxTab.Unread, "Uleste"), Tab(NativeInboxTab.Mentions, "Omtalar"), Tab(NativeInboxTab.Tasks, "Oppgåver")),
                TextBlock("Lastar innboksen …").Foreground(palette.Muted).IsVisible(state.Loading || !state.Ready && state.Error is null).AutomationId("live-inbox-loading"),
                TextBlock(state.Error ?? "").TextWrapping().Foreground(palette.Error).IsVisible(state.Error is not null).AutomationId("live-inbox-error"),
                Button("Prøv igjen", () => { if (Active()) bindings.Retry(); }).MinHeight(44).IsEnabled(!state.Loading)
                    .IsVisible(state.Error is not null).AutomationId("live-inbox-retry"),
                TextBlock(empty).TextWrapping().Foreground(palette.Muted).IsVisible(state.Ready && !state.Loading && state.Error is null && count == 0).AutomationId("live-inbox-empty"),
                TextBlock("Lista viser opptil dei 200 nyaste tilgjengelege omtalane. Ei uavklart handling kan halde på ei eldre omtale til du kontrollerer utfallet.")
                    .TextWrapping().FontSize(12).Foreground(palette.Muted).IsVisible(state.Tab == NativeInboxTab.Mentions)
            ).Padding(16, 8).Background(palette.Chrome).Grid(row: 0),
            content.IsVisible(state.Ready || count > 0).AutomationId("live-inbox-list").Grid(row: 1)
        ).Background(palette.Paper).AutomationId("live-inbox");
    }
    private static string Excerpt(string body) => string.Concat(body.EnumerateRunes().Take(320).Select(rune => rune.ToString()));
}
