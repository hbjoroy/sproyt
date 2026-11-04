using System.Text;
using System.Text.Json;

internal static class NativeClientWire
{
    internal static bool Proof(string? value) => value is { Length: 43 } && value.All(c => char.IsAsciiLetterOrDigit(c) || c is '-' or '_');
    internal static void Require(bool value) { if (!value) throw Failure(); }
    internal static InvalidOperationException Failure() => new("Native client operation failed.");
    internal static string SafeMessage(string value) => new(value.Where(c => !char.IsControl(c) || c == '\n').Take(512).ToArray());

    // This is called only for a response from the admitted socket generation.
    // Correlation is checked again here before a domain error can reject a send.
    internal static JsonElement CommandResponse(JsonElement response, string requestId)
    {
        try
        {
            Require(response.GetProperty("protocol").GetString() == "sproyt.chat.v1" &&
                response.GetProperty("request_id").GetString() == requestId);
            string type = response.GetProperty("type").GetString()!;
            Require(!string.IsNullOrEmpty(type));
            if (type != "error") return response;
            var payload = response.GetProperty("payload");
            string code = payload.GetProperty("code").GetString()!;
            Require(code is { Length: >= 1 and <= 128 } && code.All(c => char.IsAsciiLetterOrDigit(c) || c == '_'));
            string message = SafeMessage(payload.GetProperty("message").GetString() ?? "Tenesta avviste førespurnaden.");
            throw NativeCommandException.FromServer(requestId, code, message);
        }
        catch (NativeCommandException) { throw; }
        catch (Exception exception) when (exception is InvalidOperationException or KeyNotFoundException or FormatException)
        { throw NativeCommandException.Uncertain(requestId); }
    }
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
