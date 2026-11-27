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
/// <c>UrsaOtelEndpoint</c> MSBuild property). Without either, nothing is sent. With one from the
/// build, nothing is sent either until the user turns diagnostics on in Settings; builds that
/// are not releases (<c>UrsaTelemetryDefault=on</c>) start with it on. Setting the environment
/// variable yourself counts as turning it on.
/// Nothing here reads file names or contents: it sends what the code passes it.
/// </remarks>
public static class Telemetry
{
    private static ILoggerFactory? factory;
    private static ILogger? logger;

    private static readonly string? EnvEndpoint = Clean(Environment.GetEnvironmentVariable("URSA_OTEL_ENDPOINT"));

    public static string? Endpoint { get; } = EnvEndpoint ?? Clean(BuildMetadata("UrsaOtelEndpoint"));

    /// <summary>Whether this build can send diagnostics at all (it names a collector).</summary>
    public static bool Available => Endpoint is not null;

    /// <summary>Whether it sends them: the collector is known and the user agreed (or set it themselves).</summary>
    public static bool Enabled { get; private set; } = Available && (EnvEndpoint is not null || Consent());

    /// <summary>This install's telemetry id: shown in Settings, and what to search the collector for.</summary>
    public static string Id { get; } = LoadId();

    public static string Version { get; } =
        typeof(Telemetry).Assembly.GetCustomAttribute<AssemblyInformationalVersionAttribute>()?.InformationalVersion.Split('+')[0] ?? "0.0.0";

    public static void Start()
    {
        if (!Enabled || factory is not null) return;
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
        if (!Enabled || Endpoint is null) yield break;
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

    /// <summary>
    /// The user's answer from Settings. The app's own logs follow at once; the host picks it up
    /// the next time Ursa starts.
    /// </summary>
    public static void SetConsent(bool on)
    {
        var settings = LocalStore.LoadSettings();
        settings.SendDiagnostics = on;
        LocalStore.SaveSettings(settings);
        if (!Available) return;
        if (on)
        {
            Enabled = true;
            Start();
        }
        else
        {
            Info("diagnostics turned off");
            Flush();
            Enabled = EnvEndpoint is not null;
        }
    }

    private static bool Consent() =>
        LocalStore.LoadSettings().SendDiagnostics ?? string.Equals(BuildMetadata("UrsaTelemetryDefault"), "on", StringComparison.OrdinalIgnoreCase);

    private static string? BuildMetadata(string key) =>
        typeof(Telemetry).Assembly.GetCustomAttributes<AssemblyMetadataAttribute>().FirstOrDefault(a => a.Key == key)?.Value;

    private static string? Clean(string? e) => string.IsNullOrWhiteSpace(e) ? null : e.Trim().TrimEnd('/');

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
