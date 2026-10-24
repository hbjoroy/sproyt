using System.Diagnostics;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
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
    private static string Hash(string value) => Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(value))).ToLowerInvariant();

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
            Check(args.Length >= 2 && new[] { "seed", "busy", "resume", "verify", "guard", "fault_seed", "fault_verify", "worker" }.Contains(args[0]), "Supply a supported probe phase and test-owned database");
            string phase = args[0], path = Path.GetFullPath(args[1]);
            if (phase == "worker")
            {
                Check(args.Length == 2, "Worker phase takes only a database");
                WorkerProbe.Run(Call, path, Account, Channel);
                return 0;
            }
            if (phase == "guard")
            {
                Call(new { Op = "arm_fault", Handle = 0, Generation = 1, Site = "after_insert", Mode = "pause" }, "invalid_request");
                Console.WriteLine("PASS guard: ordinary library rejects diagnostic commands");
                return 0;
            }
            if (phase is "fault_seed" or "fault_verify")
            {
                Check(args.Length >= 3 && new[] { "after_insert", "before_commit", "after_commit" }.Contains(args[2]), "Invalid diagnostic site");
                string site = args[2];
                if (phase == "fault_seed")
                {
                    Check(args.Length == 3 && !File.Exists(path), "Fault seed requires a fresh database");
                    ulong handle = Open(path);
                    Call(new { Op = "save_draft", Handle = handle, Generation = 1, Channel, Body });
                    Call(new { Op = "arm_fault", Handle = handle, Generation = 1, Site = site, Mode = "pause" });
                    Call(new { Op = "enqueue", Handle = handle, Generation = 1, RequestId, Channel, Body });
                    throw new InvalidOperationException("Diagnostic hook returned instead of pausing");
                }
                Check(args.Length == 4 && args[3].Length == 64, "Fault verification requires the parent-retained payload hash");
                ulong reopened = Open(path);
                var snapshot = Snapshot(reopened);
                // Assert the recovered state BEFORE any retry can conceal it.
                if (site == "after_commit")
                {
                    Check(snapshot.GetProperty("drafts").GetArrayLength() == 0 && snapshot.GetProperty("outbox").GetArrayLength() == 1,
                        "Committed admission was incomplete");
                    Check(Hash(snapshot.GetProperty("outbox")[0].GetProperty("payload").GetString()!) == args[3], "Committed bytes changed after termination");
                }
                else
                {
                    Check(snapshot.GetProperty("outbox").GetArrayLength() == 0 && snapshot.GetProperty("drafts").GetArrayLength() == 1,
                        "Uncommitted transaction was partly recovered");
                    Check(snapshot.GetProperty("drafts")[0].GetProperty("body").GetString() == Body, "Rollback lost the draft");
                }
                Call(new { Op = "enqueue", Handle = reopened, Generation = 1, RequestId, Channel, Body });
                Call(new { Op = "enqueue", Handle = reopened, Generation = 1, RequestId, Channel, Body });
                snapshot = Snapshot(reopened);
                Check(snapshot.GetProperty("outbox").GetArrayLength() == 1 && snapshot.GetProperty("drafts").GetArrayLength() == 0,
                    "Retry did not produce exactly one complete local admission");
                Check(Hash(snapshot.GetProperty("outbox")[0].GetProperty("payload").GetString()!) == args[3], "Retried payload differs from pre-termination bytes");
                Call(new { Op = "close", Handle = reopened });
                Console.WriteLine($"PASS transaction: {site}; recovered state verified before same-ID retry");
                return 0;
            }
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
                Console.WriteLine("PROBE_HASH " + Hash(snapshot.GetProperty("outbox")[0].GetProperty("payload").GetString()!));
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
                Check(args.Length == 3 && args[2].Length == 64, "Resume requires the independently parent-retained hash");
                ulong handle = Open(path);
                var recovered = Snapshot(handle).GetProperty("outbox")[0];
                Check(recovered.GetProperty("request_id").GetString() == RequestId, "Request identity changed");
                Check(recovered.GetProperty("state").GetString() == "uncertain", "Interrupted dispatch not uncertain");
                string wire = recovered.GetProperty("payload").GetString()!;
                Check(Hash(wire) == args[2], "Persisted bytes differ from pre-kill parent proof");
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
