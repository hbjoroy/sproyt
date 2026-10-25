using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text.Json;

// Protects against plaintext at rest and concurrent cooperative processes. Other processes
// running as the same Windows user are outside this journal's security boundary.
internal sealed class WindowsProtectedJournal : IDisposable
{
    private const int MaximumBytes = 32 * 1024;
    private const uint UiForbidden = 0x1;
    private readonly object gate = new();
    private readonly string statePath;
    private FileStream? lease;

    internal WindowsProtectedJournal(string directory, bool create)
    {
        if (!OperatingSystem.IsWindows()) throw Failure();
        FileStream? acquired = null;
        try
        {
            var root = Path.GetFullPath(directory);
            if (create) Directory.CreateDirectory(root);
            else if (!Directory.Exists(root)) throw Failure();
            if ((File.GetAttributes(root) & FileAttributes.ReparsePoint) != 0) throw Failure();
            statePath = Path.Combine(root, "state.bin");
            var lockPath = Path.Combine(root, "owner.lock");
            if (File.Exists(lockPath) && (File.GetAttributes(lockPath) & FileAttributes.ReparsePoint) != 0) throw Failure();
            acquired = new FileStream(lockPath, create ? FileMode.OpenOrCreate : FileMode.Open, FileAccess.ReadWrite, FileShare.None);
            if (!create && !File.Exists(statePath)) throw Failure();
            if (File.Exists(statePath) && (File.GetAttributes(statePath) & FileAttributes.ReparsePoint) != 0) throw Failure();
            lease = acquired;
            acquired = null;
        }
        catch { try { acquired?.Dispose(); } catch { } throw Failure(); }
    }

    internal T Read<T>()
    {
        lock (gate)
        {
            byte[]? plaintext = null;
            try
            {
                RequireLease();
                using var stream = new FileStream(statePath, FileMode.Open, FileAccess.Read, FileShare.Read);
                if (stream.Length <= 0 || stream.Length > MaximumBytes) throw Failure();
                var ciphertext = new byte[(int)stream.Length];
                stream.ReadExactly(ciphertext);
                plaintext = Transform(ciphertext, protect: false);
                if (plaintext.Length == 0 || plaintext.Length > MaximumBytes) throw Failure();
                var record = JsonSerializer.Deserialize<T>(plaintext);
                if (record is null) throw Failure();
                return record;
            }
            catch { throw Failure(); }
            finally { if (plaintext != null) CryptographicOperations.ZeroMemory(plaintext); }
        }
    }

    internal void Write<T>(T record)
    {
        lock (gate)
        {
            byte[]? plaintext = null;
            string? temporary = null;
            try
            {
                RequireLease();
                if (record is null) throw Failure();
                plaintext = JsonSerializer.SerializeToUtf8Bytes(record);
                if (plaintext.Length == 0 || plaintext.Length > MaximumBytes) throw Failure();
                var ciphertext = Transform(plaintext, protect: true);
                if (ciphertext.Length == 0 || ciphertext.Length > MaximumBytes) throw Failure();
                temporary = Path.Combine(Path.GetDirectoryName(statePath)!, ".state-" + Guid.NewGuid().ToString("N") + ".tmp");
                using (var stream = new FileStream(temporary, FileMode.CreateNew, FileAccess.Write, FileShare.None, 4096, FileOptions.WriteThrough))
                {
                    stream.Write(ciphertext);
                    stream.Flush(flushToDisk: true);
                }
                if (File.Exists(statePath)) File.Replace(temporary, statePath, null);
                else File.Move(temporary, statePath);
                temporary = null;
            }
            catch { throw Failure(); }
            finally
            {
                if (plaintext != null) CryptographicOperations.ZeroMemory(plaintext);
                // Only this write's randomly named ciphertext staging file is eligible for cleanup.
                if (temporary != null) { try { File.Delete(temporary); } catch { } }
            }
        }
    }

    public void Dispose()
    {
        lock (gate)
        {
            try { lease?.Dispose(); }
            catch { throw Failure(); }
            finally { lease = null; }
        }
    }

    private void RequireLease()
    {
        if (!OperatingSystem.IsWindows() || lease == null) throw Failure();
    }

    private static InvalidOperationException Failure() => new("Protected journal operation failed.");

    private static byte[] Transform(byte[] bytes, bool protect)
    {
        // This explicit guard is also required for future callers of this helper.
        if (!OperatingSystem.IsWindows()) throw Failure();
        var input = new DataBlob { Length = bytes.Length, Data = Marshal.AllocHGlobal(bytes.Length) };
        var output = new DataBlob();
        try
        {
            Marshal.Copy(bytes, 0, input.Data, bytes.Length);
            var ok = protect
                ? CryptProtectData(ref input, null, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero, UiForbidden, out output)
                : CryptUnprotectData(ref input, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero, UiForbidden, out output);
            if (!ok || output.Length <= 0 || output.Length > MaximumBytes) throw Failure();
            var result = new byte[output.Length];
            Marshal.Copy(output.Data, result, 0, output.Length);
            return result;
        }
        finally
        {
            // DPAPI's decrypted allocation and the marshalled input can hold plaintext.
            if (input.Data != IntPtr.Zero) { Marshal.Copy(new byte[input.Length], 0, input.Data, input.Length); Marshal.FreeHGlobal(input.Data); }
            if (output.Data != IntPtr.Zero) { if (output.Length > 0) Marshal.Copy(new byte[output.Length], 0, output.Data, output.Length); _ = LocalFree(output.Data); }
        }
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct DataBlob { internal int Length; internal IntPtr Data; }

    [DllImport("crypt32.dll", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CryptProtectData(ref DataBlob input, string? description, IntPtr entropy, IntPtr reserved, IntPtr prompt, uint flags, out DataBlob output);

    [DllImport("crypt32.dll", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CryptUnprotectData(ref DataBlob input, IntPtr description, IntPtr entropy, IntPtr reserved, IntPtr prompt, uint flags, out DataBlob output);

    [DllImport("kernel32.dll")]
    private static extern IntPtr LocalFree(IntPtr memory);
}
