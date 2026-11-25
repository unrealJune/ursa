using System.Collections.Concurrent;
using System.Diagnostics;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;

namespace Ursa.App.Services;

public sealed class HostException(string message) : Exception(message);

/// <summary>
/// Drives <c>ursa-host.exe</c> over stdio: one JSON object per line each way. Every request gets
/// one response with its id; long operations send progress events with the same id first.
/// </summary>
public sealed class HostClient : IAsyncDisposable
{
    private static readonly JsonSerializerOptions Json = new() { PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower };

    private readonly ConcurrentDictionary<long, Pending> pending = new();
    private readonly SemaphoreSlim writeLock = new(1, 1);
    private Process? process;
    private long nextId;

    private sealed record Pending(TaskCompletionSource<JsonElement> Done, IProgress<(long Done, long Total)>? Progress, Action<string, JsonElement>? OnEvent);

    public event Action<string>? Exited;

    public static string? FindHost()
    {
        string beside = Path.Combine(AppContext.BaseDirectory, "ursa-host.exe");
        if (File.Exists(beside)) return beside;
        string? env = Environment.GetEnvironmentVariable("URSA_HOST_PATH");
        return env is not null && File.Exists(env) ? env : null;
    }

    public void Start()
    {
        string path = FindHost() ?? throw new HostException(
            "ursa-host.exe is missing. Reinstall Ursa, or set URSA_HOST_PATH when running from source.");
        var psi = new ProcessStartInfo(path)
        {
            UseShellExecute = false,
            CreateNoWindow = true,
            RedirectStandardInput = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            StandardOutputEncoding = new UTF8Encoding(false),
            StandardInputEncoding = new UTF8Encoding(false),
        };
        // The host exports its own logs under this install's telemetry id.
        foreach (var (k, v) in Telemetry.HostEnvironment()) psi.Environment[k] = v;
        var p = Process.Start(psi) ?? throw new HostException("could not start ursa-host.exe");
        p.EnableRaisingEvents = true;
        p.Exited += (_, _) =>
        {
            foreach (var (_, call) in pending)
                call.Done.TrySetException(new HostException("ursa-host stopped"));
            pending.Clear();
            Exited?.Invoke($"ursa-host exited ({SafeExitCode(p)})");
        };
        process = p;
        _ = Task.Run(() => ReadLoop(p));
        _ = Task.Run(() => DrainErrors(p));
    }

    private static string SafeExitCode(Process p)
    {
        try { return p.ExitCode.ToString(); } catch { return "?"; }
    }

    public async Task StopAsync()
    {
        var p = process;
        process = null;
        if (p is null) return;
        try
        {
            p.StandardInput.Close();
            using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(5));
            await p.WaitForExitAsync(cts.Token);
        }
        catch
        {
            try { p.Kill(entireProcessTree: true); } catch { }
        }
        p.Dispose();
    }

    public async ValueTask DisposeAsync() => await StopAsync();

    /// <summary>Sends <paramref name="cmd"/> with the public properties of <paramref name="args"/>.</summary>
    public async Task<JsonElement> CallAsync(string cmd, object? args = null,
        IProgress<(long Done, long Total)>? progress = null, Action<string, JsonElement>? onEvent = null)
    {
        var p = process ?? throw new HostException("ursa-host is not running");
        long id = Interlocked.Increment(ref nextId);
        var request = (args is null ? null : JsonSerializer.SerializeToNode(args, Json) as JsonObject) ?? new JsonObject();
        request["id"] = id;
        request["cmd"] = cmd;
        var call = new Pending(new TaskCompletionSource<JsonElement>(TaskCreationOptions.RunContinuationsAsynchronously), progress, onEvent);
        pending[id] = call;
        await writeLock.WaitAsync();
        try
        {
            await p.StandardInput.WriteLineAsync(request.ToJsonString());
            await p.StandardInput.FlushAsync();
        }
        catch (Exception e)
        {
            pending.TryRemove(id, out _);
            throw new HostException($"could not reach ursa-host: {e.Message}");
        }
        finally
        {
            writeLock.Release();
        }
        var result = await call.Done.Task;
        if (!result.TryGetProperty("ok", out var ok) || !ok.GetBoolean())
            throw new HostException(result.TryGetProperty("error", out var err) ? err.GetString() ?? "failed" : "failed");
        return result;
    }

    private async Task ReadLoop(Process p)
    {
        try
        {
            while (await p.StandardOutput.ReadLineAsync() is { } line)
            {
                JsonElement msg;
                try { msg = JsonDocument.Parse(line).RootElement.Clone(); }
                catch { continue; }
                if (!msg.TryGetProperty("id", out var idProp) || idProp.ValueKind != JsonValueKind.Number) continue;
                long id = idProp.GetInt64();
                if (msg.TryGetProperty("event", out var ev))
                {
                    if (pending.TryGetValue(id, out var call))
                    {
                        if (ev.GetString() == "progress")
                            call.Progress?.Report((msg.GetProperty("done").GetInt64(), msg.GetProperty("total").GetInt64()));
                        else
                            call.OnEvent?.Invoke(ev.GetString() ?? "", msg);
                    }
                    continue;
                }
                if (pending.TryRemove(id, out var done)) done.Done.TrySetResult(msg);
            }
        }
        catch { /* the process went away; Exited fails the pending calls */ }
    }

    private static async Task DrainErrors(Process p)
    {
        try
        {
            while (await p.StandardError.ReadLineAsync() is { } line)
                Debug.WriteLine("[ursa-host] " + line);
        }
        catch { }
    }
}
