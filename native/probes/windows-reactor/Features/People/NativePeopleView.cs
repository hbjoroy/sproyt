using Microsoft.UI.Reactor;
using Microsoft.UI.Reactor.Core;
using Microsoft.UI.Xaml;
using static Microsoft.UI.Reactor.Factories;

namespace Sproyt.WindowsProbe;

internal sealed record NativePeopleActions(Action<NativePersonalScope, string> QueryChanged,
    Func<NativePersonalScope, Task> Reload, Func<NativePersonalScope, string, Task> OpenDirect,
    Func<NativePersonalScope, Task> ReadBack, Action<NativePersonalScope>? DismissUncertain = null);

internal static class NativePeopleView
{
    internal static Element Render(NativePeopleSnapshot state, SproytPalette palette, NativePeopleActions actions)
    {
        if (state.Scope is not { } scope) return Empty();
        bool busy = state.Loading || state.OpeningUserId is not null;
        Element Person(NativePerson person)
        {
            string status = person.StatusExpiresAt is { } expiry && expiry <= DateTimeOffset.UtcNow ? "" :
                string.Join(" ", new[] { person.StatusEmoji, person.StatusText }.Where(value => value.Length != 0));
            return Button(VStack(3,
                TextBlock(person.DisplayName).SemiBold().TextWrapping().Foreground(palette.Ink),
                TextBlock(person.Handle is { Length: > 0 } handle ? "@" + handle : "").FontSize(12).Foreground(palette.Muted),
                TextBlock(status).TextWrapping().FontSize(12).Foreground(palette.Muted).IsVisible(status.Length != 0)),
                async () => await actions.OpenDirect(scope, person.Id))
                .MinHeight(52).Padding(12, 8).HorizontalContentAlignment(HorizontalAlignment.Stretch).HAlign(HorizontalAlignment.Stretch)
                .IsEnabled(!busy && !state.HasUncertainDirect).AutomationName("Opne samtale med " + person.DisplayName)
                .AutomationId("live-person-open-" + person.Id).WithKey(person.Id);
        }
        return ScrollViewer(VStack(8,
            TextBox(state.Query, value => actions.QueryChanged(scope, value)).PlaceholderText("Finn person eller brukarnamn")
                .MaxLength(256).MinHeight(44).AutomationName("Finn person").AutomationId("live-people-query"),
            Button(state.Loading ? "Hentar personar …" : "Oppdater personar", async () => await actions.Reload(scope))
                .MinHeight(44).IsEnabled(!busy).AutomationName("Oppdater personar").AutomationId("live-people-reload"),
            TextBlock(state.ReadError ?? "").TextWrapping().Foreground(palette.Error).IsVisible(state.ReadError is not null),
            TextBlock(state.OpenError ?? "").TextWrapping().Foreground(palette.Error).IsVisible(state.OpenError is not null),
            TextBlock("Utfallet er uvisst. Kontroll hentar berre servertilstanden og sender ingen ny førespurnad om oppretting.")
                .TextWrapping().Foreground(palette.Muted).IsVisible(state.HasUncertainDirect),
            Button("Kontroller samtalen", async () => await actions.ReadBack(scope)).MinHeight(44)
                .IsEnabled(!busy).IsVisible(state.HasUncertainDirect).AutomationName("Kontroller samtalen").AutomationId("live-people-readback"),
            Button("Vel ein annan person", () => actions.DismissUncertain?.Invoke(scope)).MinHeight(44)
                .IsEnabled(!busy).IsVisible(state.HasUncertainDirect && actions.DismissUncertain is not null)
                .AutomationName("Avslutt kontrollen og vel person på nytt").AutomationId("live-people-dismiss-uncertain"),
            TextBlock("Ingen personar passar søket.").TextWrapping().Foreground(palette.Muted)
                .IsVisible(state.Loaded && state.People.Count == 0),
            VStack(6, state.People.Select(Person).ToArray())
        ).Padding(12)).AutomationId("live-people-view");
    }
}
