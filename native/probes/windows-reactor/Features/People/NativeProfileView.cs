using Microsoft.UI.Reactor;
using Microsoft.UI.Reactor.Core;
using static Microsoft.UI.Reactor.Factories;

namespace Sproyt.WindowsProbe;

internal sealed record NativeProfileActions(Action<NativePersonalScope, string> NameChanged,
    Action<NativePersonalScope, string> StatusTextChanged, Action<NativePersonalScope, string> StatusEmojiChanged,
    Func<NativePersonalScope, Task> Reload, Func<NativePersonalScope, Task> SaveName,
    Func<NativePersonalScope, Task> SaveStatus, Func<NativePersonalScope, Task> ClearStatus,
    Func<NativePersonalScope, Task> ReadBack);

internal static class NativeProfileView
{
    internal static Element Render(NativeProfileSnapshot state, SproytPalette palette, NativeProfileActions actions)
    {
        if (state.Scope is not { } scope) return Empty();
        bool busy = state.Loading || state.Saving;
        return ScrollViewer(VStack(8,
            TextBlock("Visingsnamn").SemiBold().Foreground(palette.Ink),
            TextBox(state.NameDraft, value => actions.NameChanged(scope, value)).MinHeight(44)
                .AutomationName("Visingsnamn").AutomationId("live-profile-name"),
            Button("Lagre namn", async () => await actions.SaveName(scope)).MinHeight(44)
                .IsEnabled(state.CanSave).AutomationName("Lagre namn").AutomationId("live-profile-save-name"),
            TextBlock("Status").SemiBold().Foreground(palette.Ink).Margin(0, 12, 0, 0),
            TextBox(state.StatusTextDraft, value => actions.StatusTextChanged(scope, value)).MinHeight(44)
                .AutomationName("Statustekst").AutomationId("live-profile-status-text"),
            TextBox(state.StatusEmojiDraft, value => actions.StatusEmojiChanged(scope, value)).MinHeight(44)
                .AutomationName("Status-emoji").AutomationId("live-profile-status-emoji"),
            Button("Lagre status", async () => await actions.SaveStatus(scope)).MinHeight(44)
                .IsEnabled(state.CanSave).AutomationName("Lagre status").AutomationId("live-profile-save-status"),
            Button("Tøm status", async () => await actions.ClearStatus(scope)).MinHeight(44)
                .IsEnabled(state.CanSave).AutomationName("Tøm status").AutomationId("live-profile-clear-status"),
            TextBlock(state.Error ?? "").TextWrapping().Foreground(palette.Error).IsVisible(state.Error is not null)
                .AutomationId("live-profile-error"),
            TextBlock(state.Notice ?? "").TextWrapping().Foreground(palette.Muted).IsVisible(state.Notice is not null)
                .AutomationId("live-profile-notice"),
            TextBlock("Utfallet av siste lagring er uvisst. Kontroll hentar profilen utan å lagre på nytt.")
                .TextWrapping().Foreground(palette.Muted).IsVisible(state.HasUncertainWrite),
            Button("Kontroller lagringa", async () => await actions.ReadBack(scope)).MinHeight(44)
                .IsEnabled(!busy).IsVisible(state.HasUncertainWrite).AutomationName("Kontroller lagringa").AutomationId("live-profile-readback"),
            Button(state.Loading ? "Hentar profil …" : "Oppdater profil", async () => await actions.Reload(scope)).MinHeight(44)
                .IsEnabled(!busy).AutomationName("Oppdater profil").AutomationId("live-profile-reload")
        ).Padding(12)).AutomationId("live-profile-view");
    }
}
