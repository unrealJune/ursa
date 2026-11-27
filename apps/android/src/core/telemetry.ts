/**
 * Developer telemetry for the app's JavaScript: errors, warnings and a few app events, sent as
 * OTLP logs to the collector this build names. The Rust core exports its own logs to the same
 * place under the same telemetry id (`configureTelemetry`), so one query shows both layers of a
 * phone; `ursa.layer` tells them apart (`js` here, `core` there).
 *
 * Hand-rolled OTLP/JSON over fetch, as Street Cryptid found the OpenTelemetry JS SDK misbehaves
 * under Hermes. Inert when the build names no endpoint, and until the user turns diagnostics on
 * in the Account tab (builds that are not releases set EXPO_PUBLIC_TELEMETRY_DEFAULT=on and start
 * with it on). Nothing here reads file names or contents: it sends what the code passes it.
 */
import Constants from 'expo-constants';
import * as SecureStore from 'expo-secure-store';
import { AppState } from 'react-native';
import { Ursa } from 'ursa-core';

/** Read statically: Metro inlines `process.env.EXPO_PUBLIC_*` only when it is written out. */
const ENDPOINT = process.env.EXPO_PUBLIC_OTEL_ENDPOINT?.trim().replace(/\/+$/, '') || null;
const DEFAULT_ON = process.env.EXPO_PUBLIC_TELEMETRY_DEFAULT === 'on';
const ID_KEY = 'ursa.telemetry-id';
const CONSENT_KEY = 'ursa.telemetry-consent';
const MAX_QUEUED = 500;
const BATCH = 100;

type Severity = 'DEBUG' | 'INFO' | 'WARN' | 'ERROR';
const SEVERITY_NUMBER: Record<Severity, number> = { DEBUG: 5, INFO: 9, WARN: 13, ERROR: 17 };
type Attrs = Record<string, string | number | boolean | null | undefined>;

type Record_ = { timeUnixNano: string; severityText: Severity; severityNumber: number; body: { stringValue: string }; attributes: OtlpAttr[] };
type OtlpAttr = { key: string; value: { stringValue?: string; intValue?: string; doubleValue?: number; boolValue?: boolean } };

let instanceId: string | null = null;
let queue: Record_[] = [];
let timer: ReturnType<typeof setTimeout> | null = null;
let sending = false;
let installed = false;
/** Sending: the user agreed. The core, once configured, keeps exporting until the app restarts. */
let active = false;

/** Whether this build can send diagnostics at all (it names a collector). */
export const telemetryAvailable = ENDPOINT !== null;

/** The user's answer, or the build's default before they have given one. */
export async function telemetryConsent(): Promise<boolean> {
  const v = await SecureStore.getItemAsync(CONSENT_KEY);
  return v === null ? DEFAULT_ON : v === '1';
}

export async function setTelemetryConsent(on: boolean): Promise<void> {
  await SecureStore.setItemAsync(CONSENT_KEY, on ? '1' : '0');
  if (on) {
    await startTelemetry();
  } else if (active) {
    log('INFO', 'diagnostics turned off');
    await flush();
    active = false;
    queue = [];
  }
}

/** This install's telemetry id: shown in the Account tab, and what to search the collector for. */
export async function telemetryId(): Promise<string> {
  if (instanceId) return instanceId;
  let id = await SecureStore.getItemAsync(ID_KEY);
  if (!id) {
    id = Array.from({ length: 10 }, () => Math.floor(Math.random() * 16).toString(16)).join('');
    await SecureStore.setItemAsync(ID_KEY, id);
  }
  instanceId = id;
  return id;
}

/**
 * Starts telemetry: the core's export, then this layer's error hooks. Called once, before the
 * core starts, so the core's start-up is recorded too.
 */
export async function startTelemetry(): Promise<void> {
  if (!ENDPOINT || active || !(await telemetryConsent())) return;
  active = true;
  if (installed) return;
  installed = true;
  const id = await telemetryId();
  try {
    Ursa.configureTelemetry(ENDPOINT, id, version());
  } catch (e) {
    log('WARN', 'telemetry: the core could not start exporting', { error: String(e) });
  }
  hookErrors();
  AppState.addEventListener('change', (s) => {
    if (s !== 'active') {
      void flush();
      Ursa.flushTelemetry();
    }
  });
  log('INFO', 'app started');
}

/** Records one log line (dropped when telemetry is off). */
export function log(severity: Severity, body: string, attrs: Attrs = {}): void {
  if (!ENDPOINT || !active) return;
  queue.push({
    timeUnixNano: `${Date.now()}000000`,
    severityText: severity,
    severityNumber: SEVERITY_NUMBER[severity],
    body: { stringValue: body },
    attributes: Object.entries(attrs)
      .filter(([, v]) => v !== undefined && v !== null)
      .map(([key, v]) => ({ key, value: otlpValue(v as string | number | boolean) })),
  });
  if (queue.length > MAX_QUEUED) queue = queue.slice(-MAX_QUEUED);
  if (severity === 'ERROR') void flush();
  else if (!timer) timer = setTimeout(() => void flush(), 5000);
}

/** Sends what is queued. Kept on failure (bounded), so an offline spell loses only the oldest. */
export async function flush(): Promise<void> {
  if (timer) {
    clearTimeout(timer);
    timer = null;
  }
  if (!ENDPOINT || sending || queue.length === 0) return;
  sending = true;
  const batch = queue.slice(0, BATCH);
  try {
    const res = await fetch(`${ENDPOINT}/v1/logs`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(payload(batch, await telemetryId())),
    });
    // A refusal (4xx) will not get better by sending it again.
    if (res.ok || (res.status >= 400 && res.status < 500)) queue = queue.slice(batch.length);
  } catch {
    // Offline: try again with the next record.
  } finally {
    sending = false;
  }
  if (queue.length > 0 && !timer) timer = setTimeout(() => void flush(), 15000);
}

function version(): string {
  return Constants.expoConfig?.version ?? '0.0.0';
}

function otlpValue(v: string | number | boolean): OtlpAttr['value'] {
  if (typeof v === 'boolean') return { boolValue: v };
  if (typeof v === 'number') return Number.isInteger(v) ? { intValue: String(v) } : { doubleValue: v };
  return { stringValue: v };
}

function payload(records: Record_[], id: string) {
  const attr = (key: string, stringValue: string): OtlpAttr => ({ key, value: { stringValue } });
  return {
    resourceLogs: [
      {
        resource: {
          attributes: [
            attr('service.name', 'ursa-android'),
            attr('service.version', version()),
            attr('service.instance.id', id),
            attr('ursa.layer', 'js'),
            attr('os.type', 'android'),
            attr('telemetry.sdk.language', 'javascript'),
          ],
        },
        scopeLogs: [{ scope: { name: 'ursa-android-js' }, logRecords: records }],
      },
    ],
  };
}

function text(args: unknown[]): string {
  return args
    .map((a) => (a instanceof Error ? `${a.name}: ${a.message}` : typeof a === 'string' ? a : safeJson(a)))
    .join(' ')
    .slice(0, 4000);
}

function safeJson(v: unknown): string {
  try {
    return JSON.stringify(v);
  } catch {
    return String(v);
  }
}

/** Uncaught errors, unhandled rejections, and console.error / console.warn. */
function hookErrors() {
  const g = globalThis as unknown as {
    ErrorUtils?: { getGlobalHandler(): (e: unknown, fatal?: boolean) => void; setGlobalHandler(h: (e: unknown, fatal?: boolean) => void): void };
    HermesInternal?: { enablePromiseRejectionTracker?(o: { allRejections: boolean; onUnhandled(id: number, e: unknown): void }): void };
  };
  const previous = g.ErrorUtils?.getGlobalHandler();
  g.ErrorUtils?.setGlobalHandler((e, fatal) => {
    const err = e instanceof Error ? e : new Error(String(e));
    log('ERROR', `uncaught: ${err.name}: ${err.message}`, { fatal: !!fatal, stack: err.stack?.slice(0, 4000) });
    previous?.(e, fatal);
  });
  g.HermesInternal?.enablePromiseRejectionTracker?.({
    allRejections: true,
    onUnhandled: (_id, e) => {
      const err = e instanceof Error ? e : new Error(String(e));
      log('ERROR', `unhandled rejection: ${err.name}: ${err.message}`, { stack: err.stack?.slice(0, 4000) });
    },
  });
  const { error, warn } = console;
  console.error = (...args: unknown[]) => {
    log('ERROR', text(args));
    error(...args);
  };
  console.warn = (...args: unknown[]) => {
    log('WARN', text(args));
    warn(...args);
  };
}
