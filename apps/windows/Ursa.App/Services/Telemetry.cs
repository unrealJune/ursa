using System.Reflection;
using Microsoft.Extensions.Logging;
using OpenTelemetry.Exporter;
using OpenTelemetry.Logs;
using OpenTelemetry.Resources;

namespace Ursa.App.Services;

/// <summary>
/// Developer telemetry for the Windows app: its own errors and events as OTLP logs, to the
/// collector this build names. The Rust host it starts exports under the same telemetry id
/// (<see cref="HostEnvironment"/>), so one query shows the app and its host together;
/// <c>ursa.layer</c> tells them apart (<c>app</c> here, <c>host</c> there).
/// </summary>
/// <remarks>
/// The collector comes from <c>URSA_OTEL_ENDPOINT</c>, else from the build (CI sets the
/// <c>UrsaOtelEndpoint</c> MSBuild property). Without either, nothing is sent.
/// Nothing here reads file names or contents: it sends what the code passes it.
/// </remarks>
public static class Telemetry
{
    private static ILoggerFactory? factory;
    private static ILogger? logger;

    public static string? Endpoint { get; } = FindEndpoint();
    public static bool Enabled => Endpoint is not null;

    /// <summary>This install's telemetry id: shown in Settings, and what to search the collector for.</summary>
    public static string Id { get; } = LoadId();

    public static string Version { get; } =
        typeof(Telemetry).Assembly.GetCustomAttribute<AssemblyInformationalVersionAttribute>()?.InformationalVersion.Split('+')[0] ?? "0.0.0";

    public static void Start()
    {
        if (Endpoint is null || factory is not null) return;
        factory = LoggerFactory.Create(b => b.AddOpenTelemetry(o =>
        {
            o.IncludeFormattedMessage = true;
            o.SetResourceBuilder(ResourceBuilder.CreateEmpty()
                .AddService("ursa-windows", serviceVersion: Version, serviceInstanceId: Id, autoGenerateServiceInstanceId: false)
                .AddAttributes([
                    new("ursa.layer", "app"),
                    new("os.type", "windows"),
                ]));
            o.AddOtlpExporter(e =>
            {
                e.Endpoint = new Uri($"{Endpoint}/v1/logs");
                e.Protocol = OtlpExportProtocol.HttpProtobuf;
            });
        }));
        logger = factory.CreateLogger("Ursa.App");
        Info("app started");
    }

    /// <summary>What the host needs to export as the same install.</summary>
    public static IEnumerable<KeyValuePair<string, string>> HostEnvironment()
    {
        if (Endpoint is null) yield break;
        yield return new("OTEL_EXPORTER_OTLP_ENDPOINT", Endpoint);
        yield return new("OTEL_SERVICE_INSTANCE_ID", Id);
        yield return new("OTEL_SERVICE_VERSION", Version);
    }

    public static void Info(string message) => logger?.LogInformation("{Message}", message);
    public static void Warn(string message) => logger?.LogWarning("{Message}", message);
    public static void Error(string message, Exception? e = null) => logger?.LogError(e, "{Message}", message);

    /// <summary>Sends what is buffered; for a process about to exit.</summary>
    public static void Flush()
    {
        var f = factory;
        factory = null;
        logger = null;
        f?.Dispose();
    }

    private static string? FindEndpoint()
    {
        string? e = Environment.GetEnvironmentVariable("URSA_OTEL_ENDPOINT");
        if (string.IsNullOrWhiteSpace(e))
            e = typeof(Telemetry).Assembly.GetCustomAttributes<AssemblyMetadataAttribute>()
                .FirstOrDefault(a => a.Key == "UrsaOtelEndpoint")?.Value;
        return string.IsNullOrWhiteSpace(e) ? null : e.Trim().TrimEnd('/');
    }

    private static string LoadId()
    {
        string path = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Ursa", "telemetry-id");
        try
        {
            if (File.Exists(path))
            {
                string id = File.ReadAllText(path).Trim();
                if (id.Length > 0) return id;
            }
            string fresh = Convert.ToHexStringLower(System.Security.Cryptography.RandomNumberGenerator.GetBytes(5));
            Directory.CreateDirectory(Path.GetDirectoryName(path)!);
            File.WriteAllText(path, fresh);
            return fresh;
        }
        catch
        {
            return "unknown";
        }
    }
}
