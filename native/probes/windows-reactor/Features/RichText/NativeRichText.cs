using Markdig;
using Markdig.Extensions.EmphasisExtras;
using Markdig.Extensions.Tables;
using Markdig.Extensions.TaskLists;
using Markdig.Helpers;
using Markdig.Syntax;
using Markdig.Syntax.Inlines;
using System.Globalization;
using System.Text;

namespace Sproyt.WindowsProbe;

[Flags]
internal enum NativeTextStyle { Plain = 0, Strong = 1, Italic = 2, Strike = 4, Code = 8 }
internal enum NativeBlockKind { Paragraph, Heading, Code, DiagramSource, Dice, Table, Rule, Source }
internal sealed record NativeTextRun(string Text, NativeTextStyle Style = NativeTextStyle.Plain, Uri? Link = null);
internal sealed record NativeTableRow(bool Header, IReadOnlyList<IReadOnlyList<NativeTextRun>> Cells);
internal sealed record NativeRichBlock(NativeBlockKind Kind, IReadOnlyList<NativeTextRun> Runs,
    int Level = 0, int Depth = 0, string? Prefix = null, IReadOnlyList<NativeTableRow>? Rows = null);
internal sealed record NativeRichDocument(IReadOnlyList<NativeRichBlock> Blocks, bool AtCapacity = false);

// Interpret only the AST nodes supported by the native view. This pipeline has
// no HTML renderer, external-resource resolver, diagram engine or URI launcher.
internal static class NativeRichText
{
    internal const int MaximumSourceLength = NativeMessageContent.MaximumRichSourceLength;
    private const int MaximumBlocks = 128, MaximumRuns = 1024, MaximumTableCells = 256;
    private static readonly MarkdownPipeline Pipeline = new MarkdownPipelineBuilder { MaximumNestingDepth = 32, PreciseSourceLocation = true }
        .UsePipeTables(new PipeTableOptions { UseGfmRules = true })
        .UseEmphasisExtras(EmphasisExtraOptions.Strikethrough).UseTaskLists().UseAutoLinks().Build();

    internal static bool TryWebLink(string? url, out Uri? safe)
    {
        safe = null;
        if (string.IsNullOrWhiteSpace(url) || url.Length > 2048 || url.Any(char.IsControl) || url.Contains('\\') ||
            !Uri.TryCreate(url, UriKind.Absolute, out var uri) || uri.Scheme is not ("https" or "http") ||
            uri.Host.Length == 0 || uri.UserInfo.Length != 0) return false;
        safe = uri;
        return true;
    }

    // Wire markers inside code, raw HTML or an image/link target remain literal.
    // Use the same precise AST as rendering rather than another Markdown grammar.
    internal static IReadOnlyList<(int Start, int End)> LiteralRanges(string source)
    {
        if (source.Length > MaximumSourceLength) return [(0, source.Length - 1)];
        var ranges = new List<(int, int)>();
        try
        {
            void Visit(MarkdownObject node)
            {
                if (node is CodeBlock code)
                {
                    // An unterminated fenced block can retain only the opener
                    // in Span. Its original line slices still cover the body.
                    int end = node.Span.End;
                    for (int line = 0; line < code.Lines.Count; line++) end = Math.Max(end, code.Lines.Lines[line].Slice.End);
                    ranges.Add((node.Span.Start, end)); return;
                }
                if (node is CodeInline or HtmlBlock or HtmlInline or LinkInline or AutolinkInline)
                { ranges.Add((node.Span.Start, node.Span.End)); return; }
                if (node is LeafBlock { Inline: { } inline }) Visit(inline);
                if (node is ContainerBlock blocks) foreach (var block in blocks) Visit(block);
                if (node is ContainerInline inlines) foreach (var inlineNode in inlines) Visit(inlineNode);
            }
            Visit(Markdown.Parse(source, Pipeline));
            return ranges;
        }
        catch { return [(0, source.Length - 1)]; }
    }

    internal static bool AttachmentSuffixVisible(string wireBody, IReadOnlyList<string> attachments)
    {
        if (attachments.Count == 0) return true;
        var rendered = new HashSet<(int Start, int Length)>();
        NativeMessageContent.ParseRichText(wireBody, LiteralRanges(wireBody), rendered);
        int end = wireBody.Length;
        for (int index = attachments.Count - 1; index >= 0; index--)
        {
            string marker = attachments[index]; int start = end - marker.Length;
            if (start < 0 || !wireBody.AsSpan(start, marker.Length).SequenceEqual(marker) ||
                !rendered.Contains((start, marker.Length))) return false;
            end = start > 0 && wireBody[start - 1] == '\n' ? start - 1 : start;
        }
        return true;
    }

    internal static NativeRichDocument Parse(string source)
    {
        if (source.Length > MaximumSourceLength) return Source(source[..MaximumSourceLength]);
        try
        {
            var document = Markdown.Parse(source, Pipeline);
            var blocks = new List<NativeRichBlock>();
            int runs = 0, cells = 0;
            void Add(NativeRichBlock block)
            {
                if (blocks.Count >= MaximumBlocks) throw new CapacityException();
                blocks.Add(block);
            }
            IReadOnlyList<NativeTextRun> Inline(ContainerInline? container)
            {
                var result = new List<NativeTextRun>();
                void Text(string value, NativeTextStyle style, Uri? href)
                {
                    if (value.Length == 0) return;
                    if (++runs > MaximumRuns) throw new CapacityException();
                    result.Add(new(value, style, href));
                }
                void Children(ContainerInline parent, NativeTextStyle style, Uri? href, bool permitLinks = true)
                {
                    foreach (var item in parent)
                    {
                        switch (item)
                        {
                            case LiteralInline literal: Text(literal.Content.ToString(), style, href); break;
                            case CodeInline code: Text(code.Content, style | NativeTextStyle.Code, href); break;
                            case HtmlEntityInline entity: Text(entity.Transcoded.ToString(), style, href); break;
                            case LineBreakInline line: Text(line.IsHard ? "\n" : " ", style, href); break;
                            case TaskList task: Text(task.Checked ? "☑ " : "☐ ", style, href); break;
                            case AutolinkInline auto:
                                TryWebLink(auto.Url, out var autoUri);
                                Text(auto.Url, style, permitLinks && !auto.IsEmail ? autoUri : null);
                                break;
                            case LinkInline { IsImage: true } image:
                                Text(image.FirstChild is null ? "Eksternt bilete" : "Bilete: ", style, null);
                                Children(image, style, null, permitLinks: false);
                                break;
                            case LinkInline link:
                                TryWebLink(link.Url, out var linkUri);
                                // GFM can synthesize an http URL for a bare www name.
                                // Only explicitly addressed web links are actionable.
                                if (link.IsAutoLink && !TryWebLink(Slice(link.Span), out _)) linkUri = null;
                                Children(link, style, permitLinks ? linkUri : null, permitLinks);
                                break;
                            case EmphasisInline emphasis:
                                var extra = emphasis.DelimiterChar == '~' ? NativeTextStyle.Strike :
                                    emphasis.DelimiterCount >= 2 ? NativeTextStyle.Strong : NativeTextStyle.Italic;
                                Children(emphasis, style | extra, href, permitLinks);
                                break;
                            case ContainerInline nested: Children(nested, style, href, permitLinks); break;
                            default: Text(Slice(item.Span), style, null); break;
                        }
                    }
                }
                if (container is not null) Children(container, NativeTextStyle.Plain, null);
                return result;
            }
            string Slice(SourceSpan span) => span.Start >= 0 && span.End >= span.Start && span.End < source.Length
                ? source.Substring(span.Start, span.Length) : "";
            void Blocks(ContainerBlock parent, int depth = 0, string? prefix = null)
            {
                foreach (var block in parent)
                {
                    switch (block)
                    {
                        case HeadingBlock heading: Add(new(NativeBlockKind.Heading, Inline(heading.Inline), heading.Level, depth, prefix)); break;
                        case ParagraphBlock paragraph: Add(new(NativeBlockKind.Paragraph, Inline(paragraph.Inline), Depth: depth, Prefix: prefix)); break;
                        case CodeBlock code:
                            string literal = CodeText(code.Lines);
                            string language = (code as FencedCodeBlock)?.Info?.ToLowerInvariant() ?? "";
                            bool dice = language == "dice" && ValidDice(literal.Trim());
                            var kind = dice ? NativeBlockKind.Dice : language == "mermaid" ? NativeBlockKind.DiagramSource : NativeBlockKind.Code;
                            Add(new(kind, [new(literal, NativeTextStyle.Code)], Depth: depth, Prefix: prefix));
                            break;
                        case Table table:
                            var rows = new List<NativeTableRow>();
                            foreach (var row in table.OfType<TableRow>())
                            {
                                var values = new List<IReadOnlyList<NativeTextRun>>();
                                foreach (var cell in row.OfType<TableCell>())
                                {
                                    if (++cells > MaximumTableCells || values.Count >= 12) throw new CapacityException();
                                    values.Add(Inline(cell.OfType<ParagraphBlock>().FirstOrDefault()?.Inline));
                                }
                                rows.Add(new(row.IsHeader, values));
                            }
                            Add(new(NativeBlockKind.Table, [], Depth: depth, Prefix: prefix, Rows: rows)); break;
                        case ThematicBreakBlock: Add(new(NativeBlockKind.Rule, [], Depth: depth)); break;
                        case QuoteBlock quote: Blocks(quote, depth + 1, "│ "); break;
                        case ListBlock list:
                            int order = int.TryParse(list.OrderedStart, NumberStyles.None, CultureInfo.InvariantCulture, out var first) ? first : 1;
                            foreach (var child in list.OfType<ListItemBlock>())
                                Blocks(child, depth + 1, list.IsOrdered ? (order++).ToString(CultureInfo.InvariantCulture) + ". " : "• ");
                            break;
                        case HtmlBlock html:
                            // A raw HTML block is displayed verbatim; its attributes and
                            // any Markdown-looking links inside it remain inert.
                            Add(new(NativeBlockKind.Paragraph, [new(Slice(html.Span))], Depth: depth, Prefix: prefix));
                            break;
                        case LinkReferenceDefinitionGroup: break;
                        default: Add(new(NativeBlockKind.Source, [new(Slice(block.Span))], Depth: depth, Prefix: prefix)); break;
                    }
                    prefix = null;
                }
            }
            Blocks(document);
            return new(blocks);
        }
        catch (Exception ex) when (ex is CapacityException or ArgumentException or InvalidOperationException)
        { return Source(source); }
    }

    private sealed class CapacityException : Exception;
    private static NativeRichDocument Source(string source) => new([new(NativeBlockKind.Source, [new(source)])], AtCapacity: true);
    private static bool ValidDice(string source) => source.Length is > 0 and <= 19 && source.Split(',').All(v => v.Length == 1 && v[0] is >= '1' and <= '6');
    private static string CodeText(StringLineGroup group)
    {
        var text = new StringBuilder();
        for (int index = 0; index < group.Count; index++)
        {
            var line = group.Lines[index];
            text.Append(line.Slice.ToString());
            text.Append(line.NewLine switch { NewLine.CarriageReturnLineFeed => "\r\n", NewLine.CarriageReturn => "\r", NewLine.LineFeed => "\n", _ => "" });
        }
        return text.ToString();
    }
}
