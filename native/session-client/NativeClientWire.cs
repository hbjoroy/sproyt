using System.Text;

internal static class NativeClientWire
{
    internal static bool Proof(string? value) => value is { Length: 43 } && value.All(c => char.IsAsciiLetterOrDigit(c) || c is '-' or '_');
    internal static void Require(bool value) { if (!value) throw Failure(); }
    internal static InvalidOperationException Failure() => new("Native client operation failed.");
    internal static async Task<string> ReadAsync(HttpResponseMessage response, int maximum, CancellationToken cancellationToken)
    {
        Require(response.Content.Headers.ContentLength is null || response.Content.Headers.ContentLength <= maximum);
        using var timeout = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken); timeout.CancelAfter(TimeSpan.FromSeconds(15));
        using var input = await response.Content.ReadAsStreamAsync(timeout.Token); using var output = new MemoryStream();
        var buffer = new byte[4096]; int count;
        while ((count = await input.ReadAsync(buffer, timeout.Token)) > 0) { Require(output.Length + count <= maximum); output.Write(buffer, 0, count); }
        return new UTF8Encoding(false, true).GetString(output.ToArray());
    }
}
