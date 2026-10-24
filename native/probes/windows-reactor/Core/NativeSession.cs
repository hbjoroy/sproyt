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

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate uint AbiVersion();
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate nuint NativeCapacity();
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate int CallNative([In] byte[] input, nuint inputLength, [Out] byte[] output, nuint outputCapacity);

    private NativeSession() => _worker = new ProbeWorker(CloseNative);

    public static async Task<NativeSession> OpenAsync(string database, string libraryPath)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(database);
        ArgumentException.ThrowIfNullOrWhiteSpace(libraryPath);
        if (!Path.IsPathFullyQualified(libraryPath) || !File.Exists(libraryPath))
            throw new NativeProbeException("invalid_library_path", "The native probe library must be an absolute path to an existing file.");
        string actualLibrary = Path.GetFullPath(libraryPath);
        string actualDatabase = Path.GetFullPath(database);
        var session = new NativeSession();
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
                var opened = session.Call(new { Op = "open", Database = actualDatabase, Account = FixtureAccount });
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

    public Task SaveDraftAsync(string channel, string body) => _worker.Queue(() =>
    {
        ArgumentNullException.ThrowIfNull(channel);
        ArgumentNullException.ThrowIfNull(body);
        Call(new { Op = "save_draft", Handle = _handle, Generation = _generation, Channel = channel, Body = body });
        return true;
    });

    public Task<QueuedMessage> EnqueueAsync(string channel, string body, string requestId) => _worker.Queue(() =>
    {
        ArgumentNullException.ThrowIfNull(channel);
        ArgumentNullException.ThrowIfNull(body);
        ArgumentNullException.ThrowIfNull(requestId);
        Call(new { Op = "enqueue", Handle = _handle, Generation = _generation, RequestId = requestId, Channel = channel, Body = body });
        // The mutation returned success. A subsequent read failure cannot be
        // represented as a failed enqueue: the stable request ID may persist.
        try
        {
            var queued = ReadSnapshot().Snapshot.Outbox.SingleOrDefault(item => item.RequestId == requestId);
            if (queued is null || queued.Channel != channel || queued.Body != body)
                throw Unknown("The committed request was absent or inconsistent in its snapshot.");
            return queued;
        }
        catch (Exception exception)
        {
            throw Unknown("The enqueue completed but its stored result could not be confirmed. Retain the same request ID.", exception);
        }
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
        if (RequiredString(result, "account") != FixtureAccount ||
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
            drafts.Add(new Draft(RequiredString(draft, "channel"), RequiredString(draft, "body")));
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
                outbox.Add(new QueuedMessage(requestId, RequiredString(channel, "value"), RequiredString(content, "body"), state));
            }
            catch (JsonException exception)
            {
                throw Unknown("The stored native outbox payload was not valid JSON.", exception);
            }
        }
        if (drafts.Select(item => item.Channel).Distinct(StringComparer.Ordinal).Count() != drafts.Count ||
            outbox.Select(item => item.RequestId).Distinct(StringComparer.Ordinal).Count() != outbox.Count)
            throw Unknown("The native snapshot contained duplicate identities.");
        return (new StoreSnapshot(drafts.AsReadOnly(), outbox.AsReadOnly()), currentGeneration);
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

    private static void RequireObject(JsonElement value, string name)
    {
        if (value.ValueKind != JsonValueKind.Object) throw Unknown($"The native {name} was not an object.");
    }

    private static NativeProbeException Unknown(string message, Exception? exception = null) => new("unknown_outcome", message, exception);
}
