using System.Runtime.InteropServices;
using System.Text.Json;

namespace Sproyt.WindowsProbe;

// A local source probe over the existing C ABI, not a shipping SDK. Its sole
// worker owns library loading, every native call, and native handle cleanup.
public sealed class NativeSession : IAsyncDisposable
{
    public const string FixtureAccount = "WindowsProbe-account";
    private const int ResponseCapacity = 256 * 1024;
    private static readonly JsonSerializerOptions Json = new() { PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower };
    private readonly ProbeWorker _worker;
    private IntPtr _library;
    private CallNative? _call;
    private ulong _handle;
    private ulong _generation;
    private string _account = FixtureAccount;
    private bool _clearEmptyDraft;

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate uint AbiVersion();
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate nuint NativeCapacity();
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate int CallNative([In] byte[] input, nuint inputLength, [Out] byte[] output, nuint outputCapacity);

    private NativeSession() => _worker = new ProbeWorker(CloseNative);

    public static async Task<NativeSession> OpenAsync(string database, string libraryPath, string account = FixtureAccount, bool clearEmptyDraft = false)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(database);
        ArgumentException.ThrowIfNullOrWhiteSpace(libraryPath);
        if (!Path.IsPathFullyQualified(libraryPath) || !File.Exists(libraryPath))
            throw new NativeProbeException("invalid_library_path", "The native probe library must be an absolute path to an existing file.");
        string actualLibrary = Path.GetFullPath(libraryPath);
        string actualDatabase = Path.GetFullPath(database);
        var session = new NativeSession();
        session._account = account;
        session._clearEmptyDraft = clearEmptyDraft;
        try
        {
            await session._worker.Queue(() =>
            {
                session._library = NativeLibrary.Load(actualLibrary);
                var version = session.Export<AbiVersion>("sproyt_probe_abi_version");
                var capacity = session.Export<NativeCapacity>("sproyt_probe_response_capacity");
                if (version() != 1 || capacity() != ResponseCapacity)
                    throw new NativeProbeException("abi_mismatch", "Expected probe ABI 1 and a 256 KiB response buffer.");
                session._call = session.Export<CallNative>("sproyt_probe_call");
                // Explicit opt-in: an older ABI-1 library rejects this unknown
                // field instead of silently applying body-only draft removal.
                var opened = session.Call(new { Op = "open", Database = actualDatabase, Account = account, DraftRevisions = true, ThreadTargets = true, UploadDrafts = true });
                if (!opened.TryGetProperty("handle", out var handle) || handle.ValueKind != JsonValueKind.Number ||
                    !handle.TryGetUInt64(out session._handle) || session._handle == 0)
                    throw Unknown("The native open reply did not contain a valid handle.");
                var snapshot = session.ReadSnapshot();
                session._generation = snapshot.Generation;
                return true;
            }).ConfigureAwait(false);
            return session;
        }
        catch
        {
            await session._worker.CloseAsync().ConfigureAwait(false);
            throw;
        }
    }

    public Task<StoreSnapshot> SnapshotAsync() => _worker.Queue(() => ReadSnapshot().Snapshot);

    public Task DeliveryAsync(string requestId, string action) => _worker.Queue(() =>
    {
        Call(new { Op = "delivery", Handle = _handle, Generation = _generation, RequestId = requestId, Action = action });
        return true;
    });

    public Task<long> SaveDraftAsync(string channel, string body, string? parentMessageId = null, DraftComposer? composer = null) => _worker.Queue(() =>
    {
        ArgumentNullException.ThrowIfNull(channel);
        ArgumentNullException.ThrowIfNull(body);
        var result = _clearEmptyDraft && body.Length == 0
            ? Call(new { Op = "clear_draft", Handle = _handle, Generation = _generation, Channel = channel, ParentMessageId = parentMessageId })
            : Call(new { Op = "save_draft", Handle = _handle, Generation = _generation, Channel = channel, Body = body, ParentMessageId = parentMessageId, Composer = composer });
        return RequiredRevision(result);
    });

    public Task<QueuedMessage> EnqueueAsync(string channel, string body, string requestId, long expectedDraftRevision, string? parentMessageId = null) => _worker.Queue(() =>
    {
        ArgumentNullException.ThrowIfNull(channel);
        ArgumentNullException.ThrowIfNull(body);
        ArgumentNullException.ThrowIfNull(requestId);
        ArgumentOutOfRangeException.ThrowIfNegative(expectedDraftRevision);
        Call(new { Op = "enqueue", Handle = _handle, Generation = _generation, RequestId = requestId, Channel = channel, Body = body, ExpectedDraftRevision = expectedDraftRevision, ParentMessageId = parentMessageId });
        // The mutation returned success. A subsequent read failure cannot be
        // represented as a failed enqueue: the stable request ID may persist.
        try
        {
            var queued = ReadSnapshot().Snapshot.Outbox.SingleOrDefault(item => item.RequestId == requestId);
            if (queued is null || queued.Channel != channel || queued.Body != body || queued.ParentMessageId != parentMessageId)
                throw Unknown("The committed request was absent or inconsistent in its snapshot.");
            return queued;
        }
        catch (Exception exception)
        {
            throw Unknown("The enqueue completed but its stored result could not be confirmed. Retain the same request ID.", exception);
        }
    });

    public Task<UploadDraftResult> AttachUploadAsync(ComposerTarget target, string jobId, string operationId, string body, string marker, DraftComposer? composer = null, bool receiptOnly = false) => _worker.Queue(() =>
    {
        // A receipt can outlive its original draft. Even a huge replacement in
        // the editor must reach that lookup within the ABI request-size bound.
        // 1025 ASCII bytes guarantee the same rejection on first application.
        string boundedBody = body.Length > 1024 ? new string('x', 1025) : body;
        var result = Call(new { Op = "attach_upload", Handle = _handle, Generation = _generation, JobId = jobId, OperationId = operationId, ReceiptOnly = receiptOnly,
            Channel = target.ChannelId, target.ParentMessageId, Body = boundedBody, Marker = marker, Composer = body.Length > 1024 ? null : composer });
        if (!result.TryGetProperty("already_applied", out var replay) || replay.ValueKind is not (JsonValueKind.True or JsonValueKind.False) ||
            !result.TryGetProperty("body", out var merged) || merged.ValueKind is not (JsonValueKind.Null or JsonValueKind.String))
            throw Unknown("The native upload draft result was incomplete.");
        var applied = new UploadDraftResult(replay.GetBoolean(), RequiredString(result, "operation_id"), merged.GetString(), RequiredRevision(result));
        if (receiptOnly && !applied.AlreadyApplied) throw Unknown("Receipt lookup returned a new mutation.");
        if (!applied.AlreadyApplied && (applied.OperationId != operationId || applied.Body != JoinAttachment(body, marker)))
            throw Unknown("The committed upload draft did not match its operation.");
        return applied;
    });

    public static string JoinComposer(string text, IReadOnlyList<string> attachments)
    {
        if (attachments.Count == 0) return text;
        string separator = text.Length == 0 || text.EndsWith('\n') ? "" : "\n";
        return text + separator + string.Join('\n', attachments);
    }
    internal static string JoinAttachment(string body, string marker) =>
        body.Length == 0 || body.EndsWith('\n') ? body + marker : body + "\n" + marker;

    public Task ReconcileUploadReceiptsAsync(IReadOnlyList<string> activeJobIds) => _worker.Queue(() =>
    {
        Call(new { Op = "reconcile_upload_receipts", Handle = _handle, Generation = _generation, ActiveJobIds = activeJobIds });
        return true;
    });

    public ValueTask DisposeAsync() => new(_worker.CloseAsync());

    private T Export<T>(string name) where T : Delegate =>
        Marshal.GetDelegateForFunctionPointer<T>(NativeLibrary.GetExport(_library, name));

    private JsonElement Call(object request)
    {
        byte[] input = JsonSerializer.SerializeToUtf8Bytes(request, Json);
        byte[] output = new byte[ResponseCapacity];
        int length;
        try
        {
            length = (_call ?? throw new InvalidOperationException("The native probe was not loaded."))
                (input, (nuint)input.Length, output, (nuint)output.Length);
        }
        catch (Exception exception)
        {
            throw Unknown("The native call did not return a trustworthy outcome.", exception);
        }
        if (length <= 0 || length > output.Length)
        {
            string code = length switch { -1 => "invalid_arguments", -2 => "response_capacity", -3 => "input_too_large", _ => "unknown_outcome" };
            throw new NativeProbeException(code, $"Native probe ABI returned {length}; the call is never automatically retried.");
        }
        try
        {
            using var document = JsonDocument.Parse(output.AsMemory(0, length));
            var reply = document.RootElement;
            RequireObject(reply, "reply");
            if (!reply.TryGetProperty("ok", out var ok) || ok.ValueKind is not (JsonValueKind.True or JsonValueKind.False))
                throw Unknown("The native reply had no boolean status.");
            if (!ok.GetBoolean())
            {
                string code = RequiredString(reply, "error");
                if (string.IsNullOrWhiteSpace(code)) throw Unknown("The native failure reply had no error code.");
                if (code == "injected_unknown_outcome") throw Unknown("A diagnostic native failure occurred after commit; retain the same request ID.");
                throw new NativeProbeException(code, $"Native probe rejected the request: {code}.");
            }
            if (!reply.TryGetProperty("result", out var result)) throw Unknown("The native reply had no result.");
            RequireObject(result, "result");
            return result.Clone();
        }
        catch (JsonException exception)
        {
            throw Unknown("The native reply was not valid JSON after execution.", exception);
        }
    }

    private (StoreSnapshot Snapshot, ulong Generation) ReadSnapshot()
    {
        var result = Call(new { Op = "snapshot", Handle = _handle });
        if (!result.TryGetProperty("schema", out var schema) || schema.ValueKind != JsonValueKind.Number || !schema.TryGetInt32(out int schemaVersion) || schemaVersion != 4 ||
            !result.TryGetProperty("draft_revisions", out var revisionMode) || revisionMode.ValueKind != JsonValueKind.True ||
            !result.TryGetProperty("upload_drafts", out var uploadMode) || uploadMode.ValueKind != JsonValueKind.True ||
            !result.TryGetProperty("thread_targets", out var targetMode) || targetMode.ValueKind != JsonValueKind.True ||
            RequiredString(result, "account") != _account ||
            !result.TryGetProperty("generation", out var generation) || generation.ValueKind != JsonValueKind.Number ||
            !generation.TryGetUInt64(out ulong currentGeneration) || currentGeneration == 0 ||
            !result.TryGetProperty("active", out var active) || active.ValueKind is not (JsonValueKind.True or JsonValueKind.False))
            throw Unknown("The native snapshot identity was invalid.");
        var draftArray = RequiredArray(result, "drafts");
        var outboxArray = RequiredArray(result, "outbox");
        if (draftArray.GetArrayLength() > 8 || outboxArray.GetArrayLength() > 8)
            throw Unknown("The native snapshot exceeded the probe's item limits.");
        var drafts = new List<Draft>();
        foreach (var draft in draftArray.EnumerateArray())
        {
            RequireObject(draft, "draft");
            var parsed = new Draft(RequiredString(draft, "channel"), RequiredString(draft, "body"), RequiredRevision(draft), OptionalParent(draft));
            if (draft.TryGetProperty("attachments", out var markers))
            {
                if (markers.ValueKind != JsonValueKind.Array || !draft.TryGetProperty("composer_text", out var editor) ||
                    editor.ValueKind != JsonValueKind.String) throw Unknown("Invalid draft attachment metadata.");
                var attachments = markers.EnumerateArray().Select(m => m.ValueKind == JsonValueKind.String ? m.GetString()! :
                    throw Unknown("Invalid draft marker.")).ToArray();
                if (attachments.Length == 0 || JoinComposer(editor.GetString()!, attachments) != parsed.Body)
                    throw Unknown("Attachment metadata did not reproduce the stored draft.");
                parsed = parsed with { ComposerText = editor.GetString()!, Attachments = Array.AsReadOnly(attachments) };
            }
            drafts.Add(parsed);
        }
        var outbox = new List<QueuedMessage>();
        foreach (var item in outboxArray.EnumerateArray())
        {
            RequireObject(item, "outbox item");
            string requestId = RequiredString(item, "request_id");
            string state = RequiredString(item, "state");
            if (state is not ("queued" or "dispatching" or "uncertain" or "acknowledged" or "rejected"))
                throw Unknown("The native snapshot contained an unknown delivery state.");
            try
            {
                using var payload = JsonDocument.Parse(RequiredString(item, "payload"));
                var envelope = payload.RootElement;
                RequireObject(envelope, "outbox envelope");
                if (RequiredString(envelope, "request_id") != requestId || RequiredString(envelope, "protocol") != "sproyt.chat.v1" ||
                    RequiredString(envelope, "type") != "send_message")
                    throw Unknown("The native outbox envelope identity was inconsistent.");
                var content = RequiredObject(envelope, "payload");
                var channel = RequiredObject(content, "channel");
                if (RequiredString(channel, "type") != "id") throw Unknown("The native outbox channel was invalid.");
                outbox.Add(new QueuedMessage(requestId, RequiredString(channel, "value"), RequiredString(content, "body"), state, OptionalParent(content)));
            }
            catch (JsonException exception)
            {
                throw Unknown("The stored native outbox payload was not valid JSON.", exception);
            }
        }
        if (drafts.Select(item => item.Key).Distinct(StringComparer.Ordinal).Count() != drafts.Count ||
            outbox.Select(item => item.RequestId).Distinct(StringComparer.Ordinal).Count() != outbox.Count)
            throw Unknown("The native snapshot contained duplicate identities.");
        return (new StoreSnapshot(drafts.AsReadOnly(), outbox.AsReadOnly()), currentGeneration);
    }

    private static string? OptionalParent(JsonElement item)
    {
        if (!item.TryGetProperty("parent_message_id", out var value) || value.ValueKind == JsonValueKind.Null) return null;
        if (value.ValueKind != JsonValueKind.String || value.GetString() is not { Length: > 0 } parent ||
            System.Text.Encoding.UTF8.GetByteCount(parent) > 128 || parent.Any(char.IsControl))
            throw Unknown("The native snapshot contained an invalid thread target.");
        return parent;
    }

    private void CloseNative()
    {
        try
        {
            if (_handle != 0) Call(new { Op = "close", Handle = _handle });
        }
        finally
        {
            _handle = 0;
            _call = null;
            if (_library != IntPtr.Zero)
            {
                NativeLibrary.Free(_library);
                _library = IntPtr.Zero;
            }
        }
    }

    private static JsonElement RequiredObject(JsonElement value, string name)
    {
        if (!value.TryGetProperty(name, out var property)) throw Unknown($"The native result omitted {name}.");
        RequireObject(property, name);
        return property;
    }

    private static JsonElement RequiredArray(JsonElement value, string name)
    {
        if (!value.TryGetProperty(name, out var property) || property.ValueKind != JsonValueKind.Array)
            throw Unknown($"The native result had no {name} array.");
        return property;
    }

    private static string RequiredString(JsonElement value, string name)
    {
        if (!value.TryGetProperty(name, out var property) || property.ValueKind != JsonValueKind.String)
            throw Unknown($"The native result had no {name} string.");
        return property.GetString()!;
    }

    private static long RequiredRevision(JsonElement value)
    {
        if (!value.TryGetProperty("revision", out var revision) || revision.ValueKind != JsonValueKind.Number ||
            !revision.TryGetInt64(out long result) || result <= 0)
            throw Unknown("The native result omitted a valid durable draft revision.");
        return result;
    }

    private static void RequireObject(JsonElement value, string name)
    {
        if (value.ValueKind != JsonValueKind.Object) throw Unknown($"The native {name} was not an object.");
    }

    private static NativeProbeException Unknown(string message, Exception? exception = null) => new("unknown_outcome", message, exception);
}
