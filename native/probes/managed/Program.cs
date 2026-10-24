using System.Diagnostics;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;

internal static class Program
{
    private static readonly JsonSerializerOptions Json = new() { PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower };
    private sealed record Fixture(int Schema, string Account, string Channel, string RequestId, string Body);
    private static Fixture? _fixture;
    private static Fixture Scenario => _fixture ?? throw new InvalidOperationException("Fixture was not loaded");
    private static string Account => Scenario.Account;
    private static string RequestId => Scenario.RequestId;
    private static string Body => Scenario.Body;
    private static string Channel => Scenario.Channel;

    [DllImport("sproyt_native_probe", CallingConvention = CallingConvention.Cdecl)]
    private static extern uint sproyt_probe_abi_version();
    [DllImport("sproyt_native_probe", CallingConvention = CallingConvention.Cdecl)]
    private static extern nuint sproyt_probe_response_capacity();
    [DllImport("sproyt_native_probe", CallingConvention = CallingConvention.Cdecl)]
    private static extern int sproyt_probe_call(byte[] input, nuint inputLen, [Out] byte[] output, nuint outputCapacity);

    private static JsonElement Call(object request, string? expectedError = null)
    {
        var input = JsonSerializer.SerializeToUtf8Bytes(request, Json);
        var output = new byte[checked((int)sproyt_probe_response_capacity())];
        int length = sproyt_probe_call(input, (nuint)input.Length, output, (nuint)output.Length);
        Check(length > 0 && length <= output.Length, $"ABI failure: {length}");
        using var document = JsonDocument.Parse(output.AsMemory(0, length));
        var reply = document.RootElement;
        if (expectedError is not null)
        {
            Check(!reply.GetProperty("ok").GetBoolean() && reply.GetProperty("error").GetString() == expectedError,
                $"Expected {expectedError}");
            return default;
        }
        Check(reply.GetProperty("ok").GetBoolean(), reply.ToString());
        return reply.GetProperty("result").Clone();
    }

    private static void Check(bool condition, string message)
    {
        if (!condition) throw new InvalidOperationException(message);
    }

    private static ulong Open(string path) => Call(new { Op = "open", Database = path, Account }).GetProperty("handle").GetUInt64();
    private static JsonElement Snapshot(ulong handle) => Call(new { Op = "snapshot", Handle = handle });

    private static int Main(string[] args)
    {
        try
        {
            _fixture = JsonSerializer.Deserialize<Fixture>(File.ReadAllText(Path.Combine(AppContext.BaseDirectory, "persistence-v1.json")), Json)
                ?? throw new InvalidOperationException("Fixture was not loaded");
            Check(Scenario.Schema == 1, "Unsupported fixture version");
            string library = Environment.GetEnvironmentVariable("SPROYT_NATIVE_PROBE_LIBRARY")
                ?? throw new InvalidOperationException("Set SPROYT_NATIVE_PROBE_LIBRARY to the absolute probe library path.");
            Check(Path.IsPathFullyQualified(library), "Library path must be absolute.");
            NativeLibrary.SetDllImportResolver(Assembly.GetExecutingAssembly(), (name, _, _) =>
                name == "sproyt_native_probe" ? NativeLibrary.Load(library) : IntPtr.Zero);
            Check(sproyt_probe_abi_version() == 1, "Unexpected ABI version");
            Check(args.Length == 2 && new[] { "seed", "busy", "resume", "verify" }.Contains(args[0]), "Use seed|busy|resume|verify <test-owned database>");
            string phase = args[0], path = Path.GetFullPath(args[1]);
            if (phase == "seed")
            {
                Check(!File.Exists(path), "Seed refuses to overwrite an existing database.");
                ulong handle = Open(path);
                Call(new { Op = "save_draft", Handle = handle, Generation = 1, Channel, Body });
                Call(new { Op = "enqueue", Handle = handle, Generation = 1, RequestId, Channel, Body });
                Call(new { Op = "delivery", Handle = handle, Generation = 1, RequestId, Action = "dispatch" });
                var snapshot = Snapshot(handle);
                Check(snapshot.GetProperty("drafts").GetArrayLength() == 0, "Draft not atomically cleared");
                Check(snapshot.GetProperty("outbox")[0].GetProperty("state").GetString() == "dispatching", "No durable dispatch");
                Console.WriteLine("PASS seed: Rust DLL journaled Unicode payload and cleared draft atomically");
                Console.Out.Flush();
                // Parent kills this process while its native handle is still open.
                // No explicit close/Drop path runs. This is process termination,
                // not a simulation of power loss or a storage-controller failure.
                Thread.Sleep(Timeout.Infinite);
            }
            else if (phase == "busy")
            {
                Call(new { Op = "open", Database = path, Account }, "store_busy");
                Console.WriteLine("PASS busy: second process cannot take ownership of the open database");
            }
            else if (phase == "resume")
            {
                ulong handle = Open(path);
                var recovered = Snapshot(handle).GetProperty("outbox")[0];
                Check(recovered.GetProperty("request_id").GetString() == RequestId, "Request identity changed");
                Check(recovered.GetProperty("state").GetString() == "uncertain", "Interrupted dispatch not uncertain");
                string wire = recovered.GetProperty("payload").GetString()!;
                using var payload = JsonDocument.Parse(wire);
                Check(payload.RootElement.GetProperty("payload").GetProperty("body").GetString() == Body, "Unicode payload changed");
                Call(new { Op = "enqueue", Handle = handle, Generation = 1, RequestId, Channel, Body = "Changed" }, "request_conflict");
                Call(new { Op = "enqueue", Handle = handle, Generation = 1, RequestId, Channel, Body });
                Call(new { Op = "delivery", Handle = handle, Generation = 1, RequestId, Action = "dispatch" });
                Check(Snapshot(handle).GetProperty("outbox")[0].GetProperty("payload").GetString() == wire, "Replay bytes changed");
                Call(new { Op = "delivery", Handle = handle, Generation = 1, RequestId, Action = "acknowledge" });
                Call(new { Op = "delivery", Handle = handle, Generation = 1, RequestId, Action = "acknowledge" });
                Check(Snapshot(handle).GetProperty("outbox").GetArrayLength() == 1, "Duplicate request created");
                Call(new { Op = "apply_message", Handle = handle, Generation = 1, MessageId = "fixture-message", Revision = 1, Body = "Before", Deleted = false });
                Call(new { Op = "apply_message", Handle = handle, Generation = 1, MessageId = "fixture-message", Revision = 2, Body = "After", Deleted = false });
                Check(Snapshot(handle).GetProperty("messages")[0].GetProperty("body").GetString() == "After", "Incoming edit lost");
                Call(new { Op = "close", Handle = handle });
                Call(new { Op = "snapshot", Handle = handle }, "invalid_handle");
                Console.WriteLine("PASS resume: original bytes replayed once; duplicate ACK and incoming edit handled");
            }
            else
            {
                Call(new { Op = "open", Database = path, Account = "fixture-bob" }, "account_mismatch");
                ulong handle = Open(path);
                Check(Snapshot(handle).GetProperty("outbox")[0].GetProperty("state").GetString() == "acknowledged", "Acknowledgement not persisted");
                Call(new { Op = "invalidate", Handle = handle, Generation = 1 });
                Call(new { Op = "save_draft", Handle = handle, Generation = 1, Channel, Body = "Late callback" }, "stale_generation");
                Call(new { Op = "close", Handle = handle });
                handle = Open(path);
                var snapshot = Snapshot(handle);
                Check(!snapshot.GetProperty("active").GetBoolean() && snapshot.GetProperty("generation").GetInt32() == 2, "Logout fence not persisted");
                Check(snapshot.GetProperty("outbox").GetArrayLength() == 0, "Invalidated outbox remained visible");
                Call(new { Op = "close", Handle = handle });
                Console.WriteLine("PASS verify: persisted ACK, account boundary and logout generation fence");
            }
            return 0;
        }
        catch (Exception exception)
        {
            Console.Error.WriteLine($"Probe failed: {exception.Message}");
            return 1;
        }
    }
}
