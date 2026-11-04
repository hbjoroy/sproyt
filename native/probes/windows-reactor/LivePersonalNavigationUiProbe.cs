using Microsoft.UI.Reactor;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Automation.Provider;
using Microsoft.UI.Xaml.Controls;
using System.Text.Json;
using static Sproyt.WindowsProbe.LivePersonalUiProbe;

namespace Sproyt.WindowsProbe;

internal static class LivePersonalNavigationUiProbe
{
    internal static async Task RunAsync(FrameworkElement root, LiveUiFixture fixture)
    {
        var window = ReactorApp.PrimaryWindow!;
        var originalSize = window.NativeWindow.AppWindow.Size;
        bool resize = root.ActualWidth >= 800;
        bool active = Program.LiveWindowActive;
        var cleanup = fixture.CaptureNavigationProbeCleanup();
        var personal = fixture.PersonalForProbe;
        try
        {
            Program.LiveWindowActive = false;
            if (resize) window.SetSize(360, 720);
            await Until("compact navigation", () => root.ActualWidth is > 0 and < 800);
            await fixture.NavigateAsync(new("channel-dm"));
            await fixture.NavigateAsync(new("channel-b"));
            await Until("compact origin mounted", () => ComposerChannel(root) == "channel-b");
            await Click(root, "live-show-channel-list");
            await Until("compact list covers conversation", () => Find<TextBox>(root, "live-composer") is null);
            await OpenDirect(root, NativePersonalUiFixtureCommands.PersonTwoId, "anna-to");
            string destination = await WaitForDirect(root, fixture, NativePersonalUiFixtureCommands.PersonTwoId);

            // Retain the actual return action. Removal prunes the middle entry
            // in [channel-dm, channel-b, destination] without changing destination.
            await Until("return to existing origin", () => Find<Button>(root, "live-personal-return") is { IsEnabled: true });
            var button = Find<Button>(root, "live-personal-return")!;
            var peer = FrameworkElementAutomationPeer.CreatePeerForElement(button) ?? new ButtonAutomationPeer(button);
            var invokeReturn = (IInvokeProvider)peer.GetPattern(PatternInterface.Invoke)!;
            long visit = fixture.Capture().Navigation!.Current!.Generation;
            fixture.ExcludeChannelsForNavigationProbe(["channel-b"]);
            invokeReturn.Invoke();
            await Until("return removed for revoked origin", () => Find<Button>(root, "live-personal-return") is null);
            await Task.Yield();
            var current = fixture.Capture().Navigation;
            if (current is not { CanBack: true, Current: { } stayed } || stayed.Generation != visit || fixture.Chat.SelectedChannel != destination ||
                ComposerChannel(root) != destination || LiveChatComponent.PersonalScopeForProbe is not null)
                throw new InvalidOperationException("Stale personal Return changed destination after the middle history entry was revoked.");

            fixture.ExcludeChannelsForNavigationProbe(fixture.Chat.Channels.Select(channel => channel.Id).ToArray());
            await Until("empty directory and no navigation origin", () => fixture.Chat.Channels.Count == 0 &&
                fixture.Chat.SelectedChannel is null && fixture.Capture().Navigation?.Current is null && ComposerChannel(root) is null);
            int writes = personal.DirectExecutions, refreshes = personal.ChannelRefreshes;
            await OpenDirect(root, NativePersonalUiFixtureCommands.PersonOneId, "anna-ein");
            await WaitForDirect(root, fixture, NativePersonalUiFixtureCommands.PersonOneId);
            if (fixture.Chat.Channels.Count != 1 || personal.DirectExecutions != writes + 1 ||
                personal.ChannelRefreshes <= refreshes || Find<Button>(root, "live-personal-return") is not null)
                throw new InvalidOperationException("The first direct conversation did not open from the canonical directory without a synthetic return target.");

            if (Program.LiveUiProbeDirectory is { } directory)
                await File.WriteAllTextAsync(Path.Combine(directory, "personal-navigation-ui-result.json"), JsonSerializer.Serialize(new
                { passed = true, actualWinUi = true, compactListSource = true, revokedOriginReturn = true, firstDirectWithoutOrigin = true, syntheticFixture = true }));
        }
        finally
        {
            LiveChatComponent.ClosePersonalForProbe?.Invoke();
            await cleanup();
            if (resize) window.NativeWindow.AppWindow.Resize(originalSize);
            Program.LiveWindowActive = active;
        }
    }

    private static string? ComposerChannel(FrameworkElement root) =>
        (Find<TextBox>(root, "live-composer")?.Tag as NativeMessageActionScope)?.ChannelId;

    private static async Task OpenDirect(FrameworkElement root, string user, string handle)
    {
        await Click(root, "live-personal-people");
        await Until("mounted people filter", () => Find<TextBox>(root, "live-people-query") is { IsLoaded: true });
        Find<TextBox>(root, "live-people-query")!.Text = handle;
        await Click(root, "live-person-open-" + user);
    }

    private static async Task<string> WaitForDirect(FrameworkElement root, LiveUiFixture fixture, string user)
    {
        await Until("selected direct conversation visible", () =>
            fixture.Chat.Channels.Any(channel => channel.IsDirect && channel.DirectUserId == user && channel.Id == fixture.Chat.SelectedChannel &&
                ComposerChannel(root) == channel.Id) && LiveChatComponent.PersonalScopeForProbe is null);
        return fixture.Chat.SelectedChannel!;
    }
}
