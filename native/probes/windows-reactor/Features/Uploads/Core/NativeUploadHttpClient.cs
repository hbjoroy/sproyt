using System.Net;
using System.Net.Http.Headers;
using System.Text;
using System.Text.Json;

namespace Sproyt.WindowsProbe;

// Uses the session owner's narrow allowlisted bearer seam. Bodies are bounded
// before parsing; originals never become a whole-file array or multipart body.
internal sealed class NativeUploadHttpClient : INativeUploadTransport
{
    internal const int ChunkBytes = 1024 * 1024, MaximumReplyBytes = 16 * 1024;
    internal const long MaximumFileBytes = 1024L * 1024 * 1024;
    private readonly Func<HttpRequestMessage, NativeMediaScope, CancellationToken, Task<HttpResponseMessage>> _send;
    internal NativeUploadHttpClient(NativeAuthSession owner) : this(owner.SendUploadAsync) { }
    internal NativeUploadHttpClient(Func<HttpRequestMessage, NativeMediaScope, CancellationToken, Task<HttpResponseMessage>> send) => _send = send;

    public async Task<NativeUploadConfiguration> ConfigurationAsync(NativeUploadTarget target, CancellationToken cancellationToken)
    {
        using var request = new HttpRequestMessage(HttpMethod.Get, "/api/v1/native/media-uploads/config");
        using var json = await JsonAsync(target, request, cancellationToken); var root = json.RootElement;
        bool enabled = root.GetProperty("enabled").GetBoolean();
        long maximum = root.GetProperty("max_file_bytes").GetInt64(); int chunk = root.GetProperty("chunk_bytes").GetInt32();
        Require(maximum is > 0 and <= MaximumFileBytes && chunk == ChunkBytes);
        string[]? channels = null;
        if (root.TryGetProperty("allowed_channel_ids", out var allowed) && allowed.ValueKind != JsonValueKind.Null)
        {
            Require(allowed.ValueKind == JsonValueKind.Array);
            channels = allowed.EnumerateArray().Select(item => item.GetString()!).ToArray();
            Require(channels.All(CanonicalId) && channels.Distinct(StringComparer.Ordinal).Count() == channels.Length);
        }
        return new(enabled, maximum, chunk, channels is null ? null : Array.AsReadOnly(channels));
    }
    public async Task<NativeUploadStatus> CreateAsync(NativeUploadJob job, CancellationToken cancellationToken)
    {
        using var request = new HttpRequestMessage(HttpMethod.Post, "/api/v1/native/channels/" + job.Target.ChannelId + "/media-uploads")
        {
            Content = new StringContent(JsonSerializer.Serialize(new { request_id = job.RequestId, filename = job.Source.FileName,
                content_type = job.Source.ContentType, size_bytes = job.Source.SizeBytes }), Encoding.UTF8, "application/json")
        };
        using var json = await JsonAsync(job.Target, request, cancellationToken); return Status(json.RootElement, job);
    }
    public async Task<NativeUploadStatus> StatusAsync(NativeUploadJob job, CancellationToken cancellationToken)
    {
        using var request = new HttpRequestMessage(HttpMethod.Get, Path(job));
        using var json = await JsonAsync(job.Target, request, cancellationToken); return Status(json.RootElement, job);
    }
    public async Task<NativeUploadStatus> PutChunkAsync(NativeUploadJob job, byte[] bytes, int count, string sha256, CancellationToken cancellationToken)
    {
        Require(count is > 0 and <= ChunkBytes && count <= bytes.Length && Digest(sha256));
        using var request = new HttpRequestMessage(HttpMethod.Put, Path(job) + "/chunks/" + job.NextChunk.ToString(System.Globalization.CultureInfo.InvariantCulture))
        { Content = new ByteArrayContent(bytes, 0, count) };
        request.Content.Headers.ContentType = new MediaTypeHeaderValue("application/octet-stream");
        request.Headers.Add("x-chunk-sha256", sha256);
        using var json = await JsonAsync(job.Target, request, cancellationToken); return Status(json.RootElement, job);
    }
    public async Task<NativeUploadedAttachment> CompleteAsync(NativeUploadJob job, CancellationToken cancellationToken)
    {
        using var request = new HttpRequestMessage(HttpMethod.Post, Path(job) + "/complete");
        using var json = await JsonAsync(job.Target, request, cancellationToken); var root = json.RootElement; var media = root.GetProperty("media");
        string id = media.GetProperty("id").GetString()!, name = media.GetProperty("original_filename").GetString()!,
            type = media.GetProperty("content_type").GetString()!, hash = media.GetProperty("sha256").GetString()!;
        long size = media.GetProperty("size_bytes").GetInt64();
        Require(CanonicalId(id) && media.GetProperty("owner_id").GetString() == job.Target.UserId &&
            media.GetProperty("channel_id").GetString() == job.Target.ChannelId && name == job.Source.FileName &&
            size == job.Source.SizeBytes && Digest(hash) && hash == job.Source.Sha256 && AllowedContentType(type) &&
            root.GetProperty("url").GetString() == "/api/v1/media/" + id);
        return new(id, name, type, size, hash);
    }
    public async Task AbortAsync(NativeUploadJob job, CancellationToken cancellationToken)
    {
        using var request = new HttpRequestMessage(HttpMethod.Delete, Path(job));
        using var timeout = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken); timeout.CancelAfter(TimeSpan.FromSeconds(60));
        using var response = await _send(request, job.Target.Scope, timeout.Token);
        if (response.StatusCode == HttpStatusCode.NotFound) return;
        CheckStatus(response); Require(response.StatusCode == HttpStatusCode.NoContent);
    }

    private async Task<JsonDocument> JsonAsync(NativeUploadTarget target, HttpRequestMessage request, CancellationToken cancellationToken)
    {
        using var timeout = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken); timeout.CancelAfter(TimeSpan.FromSeconds(60));
        using var response = await _send(request, target.Scope, timeout.Token); CheckStatus(response);
        Require(response.Content.Headers.ContentLength is null or <= MaximumReplyBytes);
        using var stream = await response.Content.ReadAsStreamAsync(timeout.Token); using var result = new MemoryStream();
        byte[] buffer = new byte[4096]; int count;
        while ((count = await stream.ReadAsync(buffer, timeout.Token)) != 0)
        { Require(result.Length + count <= MaximumReplyBytes); await result.WriteAsync(buffer.AsMemory(0, count), timeout.Token); }
        return JsonDocument.Parse(result.ToArray());
    }
    private static void CheckStatus(HttpResponseMessage response)
    {
        if (response.IsSuccessStatusCode) return;
        int status = (int)response.StatusCode;
        if (status is 408 or 429 or 502 or 503 or 504) throw new IOException("Opplastingstenesta er mellombels utilgjengeleg.");
        throw new NativeUploadRejectedException(status switch
        {
            401 => "Logg inn igjen før du held fram med opplastinga.",
            403 or 404 => "Opplastinga er ikkje tilgjengeleg for denne kontoen eller samtalen.",
            409 => "Opplastinga stemmer ikkje med den lagra fila.",
            413 => "Fila eller blokka er for stor.",
            415 => "Denne filtypen er ikkje støtta.",
            507 => "Det er ikkje nok plass i medielageret. Frigjer plass og prøv igjen.",
            _ => "Opplastinga vart avvist (HTTP " + status + ")."
        }, retryable: status == 507);
    }
    private static NativeUploadStatus Status(JsonElement root, NativeUploadJob job)
    {
        var status = new NativeUploadStatus(root.GetProperty("id").GetString()!, root.GetProperty("channel_id").GetString()!,
            root.GetProperty("original_filename").GetString()!, root.GetProperty("declared_type").GetString()!,
            root.GetProperty("size_bytes").GetInt64(), root.GetProperty("received_bytes").GetInt64(), root.GetProperty("next_chunk").GetInt32(),
            root.GetProperty("state").GetString()!, root.TryGetProperty("media_id", out var media) && media.ValueKind != JsonValueKind.Null ? media.GetString() : null,
            root.GetProperty("chunk_bytes").GetInt32());
        Require(CanonicalId(status.Id) && (job.UploadId is null || status.Id == job.UploadId) && status.ChannelId == job.Target.ChannelId &&
            status.FileName == job.Source.FileName && status.ContentType == job.Source.ContentType && status.SizeBytes == job.Source.SizeBytes &&
            status.ReceivedBytes >= 0 && status.ReceivedBytes <= status.SizeBytes && status.NextChunk >= 0 &&
            status.NextChunk == (status.ReceivedBytes + ChunkBytes - 1) / ChunkBytes &&
            (status.ReceivedBytes == status.SizeBytes || status.ReceivedBytes % ChunkBytes == 0) && status.ChunkBytes == ChunkBytes &&
            status.State is "open" or "sealed" or "complete" && (status.MediaId is null || CanonicalId(status.MediaId)));
        return status;
    }
    private static string Path(NativeUploadJob job) => CanonicalId(job.UploadId) ? "/api/v1/native/media-uploads/" + job.UploadId : throw new IOException("Opplastinga manglar ein gyldig identitet.");
    internal static bool CanonicalId(string? value) => Guid.TryParseExact(value, "D", out var parsed) && parsed.ToString("D") == value;
    internal static bool Digest(string? value) => value is { Length: 64 } && value.All(character => character is >= '0' and <= '9' or >= 'a' and <= 'f');
    internal static bool AllowedContentType(string? value) => value is "application/pdf" or "application/zip" or "text/plain" or "text/csv" ||
        value is not null && value == value.ToLowerInvariant() && value != "image/svg+xml" && (value.StartsWith("image/", StringComparison.Ordinal) || value.StartsWith("video/", StringComparison.Ordinal)) &&
        value.Length <= 128 && value.All(character => char.IsAsciiLetterOrDigit(character) || character is '/' or '-' or '+' or '.');
    private static void Require(bool value) { if (!value) throw new IOException("Tenesta svarte med ugyldige opplastingsdata."); }
}
