using Microsoft.UI.Reactor;
using Microsoft.UI.Reactor.Core;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Documents;
using Microsoft.UI.Xaml.Media;
using Windows.UI.Text;
using static Microsoft.UI.Reactor.Factories;
using NativeGrid = Microsoft.UI.Xaml.Controls.Grid;
using TextBlock = Microsoft.UI.Xaml.Controls.TextBlock;

namespace Sproyt.WindowsProbe;

internal static class NativeRichTextView
{
    internal static Element Render(string source, SproytPalette palette, Action<Uri>? openLink = null) =>
        Component<NativeRichTextComponent, NativeRichTextProps>(new(source, palette, openLink));
}

internal sealed record NativeRichTextProps(string Source, SproytPalette Palette, Action<Uri>? OpenLink);

internal sealed class NativeRichTextComponent : Component<NativeRichTextProps>
{
    public NativeRichTextComponent() { }

    public override Element Render() => Border(Empty()).OnMount(element =>
    {
        if (element is not Border host) return;
        var document = NativeRichText.Parse(Props.Source);
        var content = new StackPanel { Spacing = 8 };
        AutomationProperties.SetAutomationId(content, "native-rich-text");
        if (document.AtCapacity) content.Children.Add(Label("Omfangsrikt innhald blir vist som kjelde."));
        foreach (var block in document.Blocks)
        {
            var control = Block(block);
            control.Margin = new Thickness(Math.Min(block.Depth, 4) * 10, 0, 0, 0);
            content.Children.Add(control);
        }
        host.Child = content;
    }).WithKey((Props.Palette == SproytPalette.Dark ? "dark:" : "light:") + (Props.OpenLink is null ? "inert:" : "links:") + Props.Source);

    private FrameworkElement Block(NativeRichBlock block)
    {
        switch (block.Kind)
        {
            case NativeBlockKind.Table: return Table(block.Rows!);
            case NativeBlockKind.Rule: return new Border { Height = 1, Background = Props.Palette.Rule };
            case NativeBlockKind.Code:
            case NativeBlockKind.DiagramSource:
            case NativeBlockKind.Source:
                var stack = new StackPanel { Spacing = 4 };
                if (block.Kind == NativeBlockKind.DiagramSource) stack.Children.Add(Label("Diagramkjelde (Mermaid)"));
                stack.Children.Add(Text(block.Runs, block.Prefix, mono: true));
                var code = new Border { Child = stack, Background = Props.Palette.Chrome, Padding = new Thickness(8) };
                AutomationProperties.SetAutomationId(code, block.Kind == NativeBlockKind.DiagramSource ? "native-rich-diagram-source" : "native-rich-source");
                return code;
            case NativeBlockKind.Dice:
                string[] values = block.Runs[0].Text.Trim().Split(',');
                var dice = Text([new(string.Join(" ", values.Select(v => (char)('⚀' + v[0] - '1'))))], block.Prefix);
                dice.FontSize = 26;
                AutomationProperties.SetName(dice, "Terningkast: " + string.Join(" og ", values));
                return dice;
            default:
                var text = Text(block.Runs, block.Prefix);
                if (block.Kind == NativeBlockKind.Heading)
                {
                    text.FontSize = block.Level switch { 1 => 22, 2 => 20, _ => 17 };
                    text.FontWeight = Microsoft.UI.Text.FontWeights.SemiBold;
                    AutomationProperties.SetHeadingLevel(text, (AutomationHeadingLevel)block.Level);
                }
                return text;
        }
    }

    private TextBlock Label(string text) => new()
    {
        Text = text, FontSize = 12, Foreground = Props.Palette.Muted, TextWrapping = TextWrapping.Wrap
    };

    private TextBlock Text(IReadOnlyList<NativeTextRun> runs, string? prefix = null, bool mono = false)
    {
        var text = new TextBlock
        {
            FontSize = 16, Foreground = Props.Palette.Ink, IsTextSelectionEnabled = true, TextWrapping = TextWrapping.Wrap
        };
        if (mono) text.FontFamily = new FontFamily("Cascadia Mono");
        if (!string.IsNullOrEmpty(prefix)) text.Inlines.Add(new Run { Text = prefix });
        Hyperlink? activeLink = null;
        Uri? target = null;
        foreach (var part in runs)
        {
            var run = new Run { Text = part.Text };
            if (part.Style.HasFlag(NativeTextStyle.Strong)) run.FontWeight = Microsoft.UI.Text.FontWeights.Bold;
            if (part.Style.HasFlag(NativeTextStyle.Italic)) run.FontStyle = FontStyle.Italic;
            if (part.Style.HasFlag(NativeTextStyle.Code)) run.FontFamily = new FontFamily("Cascadia Mono");
            if (part.Style.HasFlag(NativeTextStyle.Strike)) run.TextDecorations = TextDecorations.Strikethrough;
            if (part.Link is { } uri && Props.OpenLink is not null)
            {
                if (activeLink is null || uri != target)
                {
                    target = uri;
                    activeLink = new Hyperlink();
                    // No NavigateUri: the host owns explicit user navigation. Read
                    // current props on click, including after a credential change.
                    activeLink.Click += (_, _) => Props.OpenLink?.Invoke(uri);
                    text.Inlines.Add(activeLink);
                }
                activeLink.Inlines.Add(run);
            }
            else
            {
                activeLink = null; target = null;
                text.Inlines.Add(run);
            }
        }
        return text;
    }

    private FrameworkElement Table(IReadOnlyList<NativeTableRow> rows)
    {
        var grid = new NativeGrid();
        AutomationProperties.SetName(grid, "Tabellinnhald");
        int width = rows.Select(row => row.Cells.Count).DefaultIfEmpty(0).Max();
        for (int col = 0; col < width; col++)
            grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto, MinWidth = 64, MaxWidth = 220 });
        for (int row = 0; row < rows.Count; row++)
        {
            grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
            for (int col = 0; col < rows[row].Cells.Count; col++)
            {
                var content = Text(rows[row].Cells[col]);
                if (rows[row].Header) content.FontWeight = Microsoft.UI.Text.FontWeights.SemiBold;
                var cell = new Border
                {
                    Child = content, Padding = new Thickness(6, 4, 6, 4), BorderBrush = Props.Palette.Rule,
                    BorderThickness = new Thickness(0, 0, 0, 1), Background = rows[row].Header ? Props.Palette.Chrome : Props.Palette.Paper
                };
                NativeGrid.SetColumn(cell, col); NativeGrid.SetRow(cell, row); grid.Children.Add(cell);
            }
        }
        var scroll = new ScrollViewer
        {
            Content = grid, HorizontalScrollBarVisibility = ScrollBarVisibility.Auto, HorizontalScrollMode = ScrollMode.Enabled,
            VerticalScrollBarVisibility = ScrollBarVisibility.Disabled, VerticalScrollMode = ScrollMode.Disabled
        };
        AutomationProperties.SetName(scroll, "Tabell");
        AutomationProperties.SetAutomationId(scroll, "native-rich-table");
        return scroll;
    }
}
