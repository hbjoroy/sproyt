using Microsoft.UI.Reactor;
using Microsoft.UI.Reactor.Core;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using static Microsoft.UI.Reactor.Factories;

namespace Sproyt.WindowsProbe;

internal sealed record NativeNavigationActions(Action<string> QueryChanged,
    Action<NativeNavigationDirectoryScope> ScopeChanged, Action<NativeNavigationTarget> Select,
    Action Back, Action Forward, Action<bool>? InteractionChanged = null);

// The integrator supplies the application-owned controller's snapshot/actions.
// This renderer never creates a transport, edits a draft or filters the canonical
// list in place. Automation IDs retain the existing real-UI probe convention.
internal static class NativeNavigationView
{
    internal static Element Render(NativeNavigationSnapshot state, SproytPalette palette, NativeNavigationActions actions)
    {
        Element Channel(NativeChannel channel) => Button(
            Grid([GridSize.Star(), GridSize.Auto], [GridSize.Auto],
                TextBlock((channel.IsDirect ? "" : "# ") + channel.Name).FontSize(16).TextWrapping().Foreground(palette.Ink).Grid(column: 0),
                // Sequences include replies and tombstones. This is a boolean
                // server-state indicator, never a fabricated message count.
                TextBlock("●").FontSize(12).Foreground(palette.Ink).Margin(8, 0, 0, 0).IsVisible(channel.LatestSequence > channel.LastReadSequence)
                    .AutomationName("Uleste meldingar").AutomationId("live-unread-" + channel.Id).Grid(column: 1)),
            () => actions.Select(new(channel.Id)))
            .HorizontalContentAlignment(HorizontalAlignment.Stretch).HAlign(HorizontalAlignment.Stretch)
            .MinHeight(44).Padding(12, 8).Margin(4, 0)
            .Background(channel.Id == state.Current?.Target.ChannelId ? palette.Selection : palette.Chrome)
            .BorderThickness(new Thickness(0)).CornerRadius(2)
            .AutomationName("Opne " + channel.Name + (channel.LatestSequence > channel.LastReadSequence ? ", uleste meldingar" : ""))
            .AutomationId("live-channel-" + channel.Id);
        Element Scope(string title, NativeNavigationDirectoryScope scope, int matches) =>
            Button(state.Query.Trim().Length > 0 ? $"{title} ({matches})" : title, () => actions.ScopeChanged(scope))
                .MinHeight(44).Padding(10, 6).Background(state.DirectoryScope == scope ? palette.Selection : palette.Chrome)
                .AutomationName(title).AutomationId("live-navigation-scope-" + scope.ToString().ToLowerInvariant());
        string empty = state.Query.Trim().Length > 0 ? "Ingen samtalar passar søket." :
            state.DirectoryScope == NativeNavigationDirectoryScope.Direct ? "Ingen direktemeldingar enno." : "Ingen kanalar enno.";
        return ScrollViewer(VStack(4,
            HStack(8,
                Button("←", actions.Back).MinHeight(44).MinWidth(44).IsEnabled(state.CanBack).AutomationName("Tilbake").AutomationId("live-navigation-back"),
                Button("→", actions.Forward).MinHeight(44).MinWidth(44).IsEnabled(state.CanForward).AutomationName("Fram").AutomationId("live-navigation-forward")
            ).Margin(12, 4),
            TextBox(state.Query, actions.QueryChanged).PlaceholderText("Finn samtale").MinHeight(44).Margin(12, 4)
                .AutomationName("Finn samtale").AutomationId("live-navigation-search"),
            HStack(4, Scope("Kretsar", NativeNavigationDirectoryScope.Circles, state.CircleMatches),
                Scope("Direkte", NativeNavigationDirectoryScope.Direct, state.DirectMatches)).Margin(12, 4),
            TextBlock(state.Error ?? "").Foreground(palette.Error).TextWrapping().Margin(12, 4).IsVisible(state.Error is not null),
            TextBlock(state.PreferenceError ?? "").Foreground(palette.Muted).TextWrapping().Margin(12, 4).IsVisible(state.PreferenceError is not null),
            VStack(4, state.Groups.SelectMany(group => new Element[]
                {
                    TextBlock(group.Name.ToUpperInvariant()).FontFamily("Cascadia Mono").FontSize(12).Foreground(palette.Muted)
                        .Margin(16, 16, 12, 6).AutomationId("live-group-" + group.Id)
                }.Concat(group.Channels.Select(Channel))).ToArray()),
            TextBlock(empty).TextWrapping().Foreground(palette.Muted).Margin(16, 12).IsVisible(state.Groups.Count == 0)
        )).Background(palette.Chrome).AutomationId("live-navigation")
            .OnMount(element =>
            {
                if (element is FrameworkElement native && actions.InteractionChanged is not null)
                {
                    var activity = new NavigationInteraction(native, actions.InteractionChanged);
                    native.Tag = activity;
                }
            })
            .OnUnmount(element =>
            {
                if (element is FrameworkElement { Tag: NavigationInteraction activity } native)
                { activity.Dispose(); native.Tag = null; }
            });
    }

    private sealed class NavigationInteraction : IDisposable
    {
        private readonly FrameworkElement _root;
        private readonly Action<bool> _changed;
        private bool _pointer, _disposed;
        internal NavigationInteraction(FrameworkElement root, Action<bool> changed)
        {
            _root = root; _changed = changed;
            root.PointerEntered += Entered; root.PointerExited += Exited;
            root.GotFocus += Focused; root.LostFocus += FocusLost;
        }
        private void Entered(object sender, Microsoft.UI.Xaml.Input.PointerRoutedEventArgs args) { _pointer = true; Apply(); }
        private void Exited(object sender, Microsoft.UI.Xaml.Input.PointerRoutedEventArgs args) { _pointer = false; Apply(); }
        private void Focused(object sender, RoutedEventArgs args) => Apply();
        private void FocusLost(object sender, RoutedEventArgs args) => _root.DispatcherQueue.TryEnqueue(Apply);
        private void Apply()
        {
            if (_disposed) return;
            var focus = _root.XamlRoot is null ? null : Microsoft.UI.Xaml.Input.FocusManager.GetFocusedElement(_root.XamlRoot) as DependencyObject;
            while (focus is not null && !ReferenceEquals(focus, _root)) focus = Microsoft.UI.Xaml.Media.VisualTreeHelper.GetParent(focus);
            _changed(_pointer || focus is not null);
        }
        public void Dispose()
        {
            if (_disposed) return; _disposed = true;
            _root.PointerEntered -= Entered; _root.PointerExited -= Exited;
            _root.GotFocus -= Focused; _root.LostFocus -= FocusLost;
            _changed(false);
        }
    }
}
