using Sproyt.WindowsProbe;
using System.Runtime.CompilerServices;
using System.Text.Json;

var checks = new List<string>();
void Check(string name, Action verify) { verify(); checks.Add(name); Console.WriteLine("PASS " + name); }
static void Require(bool condition, [CallerArgumentExpression(nameof(condition))] string? expression = null)
{
    if (!condition) throw new InvalidOperationException(expression);
}
static string Text(IEnumerable<NativeTextRun> runs) => string.Concat(runs.Select(r => r.Text));
static string Body(NativeRichBlock block) => Text(block.Runs);

Check("headings, nested emphasis, strike and soft breaks", () =>
{
    var parsed = NativeRichText.Parse("# Sprøyt\n\n**sterkt _begge_** ~~før~~\nså");
    Require(!parsed.AtCapacity && parsed.Blocks.Count == 2);
    Require(parsed.Blocks[0].Kind == NativeBlockKind.Heading && parsed.Blocks[0].Level == 1);
    var runs = parsed.Blocks[1].Runs;
    Require(Text(runs) == "sterkt begge før så");
    Require(runs.Any(r => r.Text == "begge" && r.Style == (NativeTextStyle.Strong | NativeTextStyle.Italic)));
    Require(runs.Any(r => r.Text == "før" && r.Style.HasFlag(NativeTextStyle.Strike)));
});
Check("GFM pipe table with semantic heading and safe link", () =>
{
    var block = NativeRichText.Parse("| Stad | Status |\n| --- | ---: |\n| [Noreg](<https://example.org/a>) | ~~utgått~~ |").Blocks.Single();
    Require(block.Kind == NativeBlockKind.Table && block.Rows!.Count == 2 && block.Rows[0].Header && !block.Rows[1].Header);
    Require(Text(block.Rows![0].Cells[0]) == "Stad");
    Require(block.Rows[1].Cells[0].Single().Link?.AbsoluteUri == "https://example.org/a");
    Require(block.Rows[1].Cells[1].Single().Style == NativeTextStyle.Strike);
});
Check("ordered items, nested bullets, tasks and quotes", () =>
{
    var blocks = NativeRichText.Parse("3. Ein\n4. To\n   - [x] Ferdig\n\n> Sitat\n\n---").Blocks;
    Require(blocks[0].Prefix == "3. " && blocks[1].Prefix == "4. ");
    Require(blocks[2].Depth == 2 && Body(blocks[2]).Contains("☑") && blocks[2].Prefix == "• ");
    Require(Body(blocks[3]) == "Sitat" && blocks[3].Depth == 1);
    Require(blocks[4].Kind == NativeBlockKind.Rule);
});
Check("explicit web actions only, images stay text", () =>
{
    string[] denied = ["file:///C:/private", "data:text/html,x", "javascript:alert(1)", "https://a:b@example.org/",
        "//example.org/x", "/relative/path", "ms-settings:privacy", "http:\\\\evil.test", "https://exa\nmple.org"];
    foreach (string url in denied) Require(!NativeRichText.TryWebLink(url, out _));
    var bare = NativeRichText.Parse("www.example.org og https://example.org/a").Blocks.Single().Runs;
    Require(bare.Count(r => r.Link != null) == 1 && bare.Single(r => r.Link != null).Link!.AbsoluteUri == "https://example.org/a");
    var runs = NativeRichText.Parse("[e-post](mailto:a@example.org) [trygg](https://example.org) ![alttekst](https://external.test/private.png)").Blocks.Single().Runs;
    Require(runs.Count(r => r.Link != null) == 1 && runs.Single(r => r.Link != null).Link!.Host == "example.org");
    Require(Text(runs).Contains("Bilete: alttekst") && !Text(runs).Contains("external.test"));
});
Check("literal HTML, code markers and exact fenced line endings", () =>
{
    const string token = "[[media:00000000-0000-0000-0000-000000000001|image/jpeg|A%20B.jpg]]";
    var inline = NativeRichText.Parse("Før `" + token + "` etter").Blocks.Single().Runs;
    Require(inline.Single(r => r.Style == NativeTextStyle.Code).Text == token);
    var html = NativeRichText.Parse("<script>\n[actie](https://example.org)\n</script>").Blocks.Single();
    Require(html.Runs.All(r => r.Link == null) && Body(html).Contains("[actie](https://example.org)"));
    var source = NativeRichText.Parse("~~~~csharp\r\n" + token + "\r\n<script>no()</script>\r\n~~~~");
    Require(source.Blocks.Single().Kind == NativeBlockKind.Code);
    Require(Body(source.Blocks[0]) == token + "\r\n<script>no()</script>\r\n");
    var raw = NativeRichText.Parse("<img src=x onerror=boom()> <script>alert(1)</script>").Blocks.Single();
    Require(Body(raw).Contains("<script>alert(1)</script>") && raw.Runs.All(r => r.Link == null));
});
Check("Mermaid source is inert and dice are bounded", () =>
{
    var diagram = NativeRichText.Parse("```mermaid\ngraph LR\n A-->B\n click A \"javascript:bad()\"\n```").Blocks.Single();
    Require(diagram.Kind == NativeBlockKind.DiagramSource && Body(diagram).Contains("javascript:bad()") && diagram.Runs.All(r => r.Link == null));
    Require(NativeRichText.Parse("```dice\n4,2\n```").Blocks.Single().Kind == NativeBlockKind.Dice);
    Require(NativeRichText.Parse("```dice\n9,2\n```").Blocks.Single().Kind == NativeBlockKind.Code);
});
Check("complexity limits preserve a readable source fallback", () =>
{
    string paragraphs = string.Join("\n\n", Enumerable.Repeat("abc", 129));
    var document = NativeRichText.Parse(paragraphs);
    Require(document.AtCapacity && document.Blocks.Single().Kind == NativeBlockKind.Source && Body(document.Blocks[0]) == paragraphs);
    string wideTable = "| a " + string.Concat(Enumerable.Repeat("| c ", 12)) + "|\n" + string.Concat(Enumerable.Repeat("| --- ", 13)) + "|";
    Require(NativeRichText.Parse(wideTable).AtCapacity);
    var lengthy = NativeRichText.Parse(new string('x', 65537));
    Require(lengthy.AtCapacity && Body(lengthy.Blocks.Single()).Length == NativeRichText.MaximumSourceLength);
    var nested = NativeRichText.Parse(new string('>', 1000) + " text");
    Require(nested.Blocks.Count > 0);
});
Check("managed attachment suffix is outside literal Markdown", () =>
{
    const string marker = "[[media:00000000-0000-0000-0000-000000000001|image/png|image.png]]";
    foreach (string opening in new[] { "```csharp\n", "~~~~\n", "<script>\n", "<!--\n" })
        Require(!NativeRichText.AttachmentSuffixVisible(opening + marker, [marker]));
    Require(NativeRichText.AttachmentSuffixVisible("```csharp\ntext\n```\n" + marker, [marker]));
    var literal = NativeRichText.LiteralRanges("`" + marker + "`\n\n" + marker);
    Require(literal.Count == 1 && literal[0].Start == 0 && literal[0].End < marker.Length + 3);
});
Check("bounded wire parsing and exact rendered attachment suffixes", () =>
{
    const string marker = "[[media:00000000-0000-0000-0000-000000000001|image/png|a]]";
    var chips = Enumerable.Repeat(marker, 17).ToArray();
    string body = string.Join('\n', chips);
    Require(System.Text.Encoding.UTF8.GetByteCount(body) <= 1024);
    Require(NativeMessageContent.ParseRichText(body, []).Count(part => part.Kind == "media") == 17);
    Require(NativeRichText.AttachmentSuffixVisible(body, chips));
    Require(!NativeRichText.AttachmentSuffixVisible(string.Join('\n', Enumerable.Repeat(marker, 21)), [marker]));
    Require(!NativeRichText.AttachmentSuffixVisible("[[media:bad|image/png|a]]", ["[[media:bad|image/png|a]]"]));
    var oversized = string.Concat(Enumerable.Repeat(marker, 1500));
    var fallback = NativeMessageContent.ParseRichText(oversized, []);
    Require(fallback is [{ Kind: "text" }] && ReferenceEquals(fallback[0].Text, oversized));
});
if (args.Length == 1 && Path.IsPathFullyQualified(args[0]))
{
    Directory.CreateDirectory(Path.GetDirectoryName(args[0])!);
    await File.WriteAllTextAsync(args[0], JsonSerializer.Serialize(new { scope = "native_rich_text_contracts", passed = true, checks }, new JsonSerializerOptions { WriteIndented = true }));
}
else throw new ArgumentException("Supply an absolute path for the receipt.");


// Only needed by the channel-group presentation helper in NativeMessageContent.
internal sealed record NativeChannel(string Id, string Name, string? CircleName = null, string? CircleId = null, bool IsDirect = false);
