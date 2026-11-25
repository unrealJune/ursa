import { useFocusEffect } from 'expo-router';
import { Fragment, useCallback, useEffect, useState } from 'react';
import { RefreshControl, ScrollView, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import Svg, { Circle, Line, Polygon, Polyline, Text as SvgText } from 'react-native-svg';
import { errorText } from 'ursa-core';

import { loadStorage, onSwitchEnded, type StorageReport } from '@/core/storage';
import { bytes } from '@/format';
import { useTheme } from '@/theme';
import { Divider, Notice, Progress, SectionHeader, Txt, withAlpha } from '@/ui/kit';
import { Bar, Card, Legend, Row } from '@/ui/meters';

/** What you store, what it takes in the pool, what you give back, and how it grows. */
export default function Usage() {
  const { c } = useTheme();
  const insets = useSafeAreaInsets();
  const [report, setReport] = useState<StorageReport | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setReport(await loadStorage());
      setError(null);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useFocusEffect(
    useCallback(() => {
      load();
    }, [load]),
  );
  useEffect(() => onSwitchEnded(load), [load]);

  if (!report) {
    return (
      <View style={{ flex: 1, backgroundColor: c.surface, padding: 16 }}>
        {error ? <Notice tone="error" text={error} /> : <Progress progress={null} />}
      </View>
    );
  }

  const r = report;
  const series = [c.primary, withAlpha(c.primary, 0.6), withAlpha(c.primary, 0.3)];
  const total = Math.max(r.limitBytes, r.storedBytes, 1);
  const friendsOnYou = r.hosting.reduce((a, h) => a + h.usedBytes, 0);

  return (
    <ScrollView
      style={{ flex: 1, backgroundColor: c.surface }}
      contentContainerStyle={{ paddingBottom: insets.bottom + 24, gap: 8 }}
      refreshControl={<RefreshControl refreshing={loading} onRefresh={load} colors={[c.primary]} progressBackgroundColor={c.surfaceContainerHigh} />}
    >
      {error ? (
        <View style={{ paddingHorizontal: 16 }}>
          <Notice tone="error" text={error} />
        </View>
      ) : null}

      <Card style={{ marginTop: 8, gap: 12 }}>
        <View style={{ flexDirection: 'row', alignItems: 'baseline', gap: 8 }}>
          <Txt role="displaySmall">{bytes(r.storedBytes)}</Txt>
          <Txt role="titleMedium" color={c.onSurfaceVariant}>{`of ${bytes(r.limitBytes)}`}</Txt>
        </View>
        <Bar
          segments={[
            { value: r.kinds.photos, color: series[0] },
            { value: r.kinds.videos, color: series[1] },
            { value: r.kinds.files, color: series[2] },
          ]}
          total={total}
          height={16}
        />
        <Legend
          items={[
            { label: `Photos ${bytes(r.kinds.photos)}`, color: series[0] },
            { label: `Videos ${bytes(r.kinds.videos)}`, color: series[1] },
            { label: `Files ${bytes(r.kinds.files)}`, color: series[2] },
            { label: `Free ${bytes(Math.max(0, r.limitBytes - r.storedBytes))}`, color: c.surfaceContainerHighest },
          ]}
        />
      </Card>

      <Card>
        <Txt role="titleSmall" style={{ marginBottom: 4 }}>
          In the pool
        </Txt>
        <Row label="Your node" value={bytes(r.homeBytes)} />
        <Row label="Friends’ nodes" value={bytes(r.friendsBytes)} />
        <Divider />
        <Row label="Total" value={bytes(r.homeBytes + r.friendsBytes)} strong />
      </Card>

      <Card style={{ gap: 6 }}>
        <Txt role="titleSmall">Fair share</Txt>
        <Row label="On friends" value={`${bytes(r.friendsBytes)} of ${bytes(r.giveBytes)}`} />
        <Bar segments={[{ value: r.friendsBytes, color: c.primary }]} total={Math.max(r.giveBytes, r.friendsBytes, 1)} height={10} />
        <Row label="Friends on you" value={bytes(friendsOnYou)} />
      </Card>

      <SectionHeader>Your pieces</SectionHeader>
      <Card style={{ gap: 10 }}>
        {r.members.map((m, i) => (
          <Fragment key={`${m.name}-${i}`}>
            <LimitRow name={m.own ? 'You' : m.name} used={m.usedBytes} limit={m.givesYou} />
          </Fragment>
        ))}
      </Card>

      {r.hosting.length ? (
        <>
          <SectionHeader>Hosting</SectionHeader>
          <Card style={{ gap: 10 }}>
            {r.hosting.map((h, i) => (
              <LimitRow key={`${h.name}-${i}`} name={h.name} used={h.usedBytes} limit={h.quotaBytes} />
            ))}
          </Card>
        </>
      ) : null}

      {r.disk ? (
        <>
          <SectionHeader>{`Disk · ${bytes(r.disk.totalBytes)}`}</SectionHeader>
          <Card style={{ gap: 10 }}>
            <Bar
              segments={[
                { value: r.disk.yoursBytes, color: series[0] },
                { value: r.disk.friendsBytes, color: series[1] },
                { value: r.disk.reservedBytes, color: series[2] },
                { value: Math.max(0, r.disk.totalBytes - r.disk.freeBytes - r.disk.yoursBytes - r.disk.friendsBytes), color: c.outline },
              ]}
              total={r.disk.totalBytes}
              height={16}
            />
            <Legend
              items={[
                { label: `Yours ${bytes(r.disk.yoursBytes)}`, color: series[0] },
                { label: `Friends ${bytes(r.disk.friendsBytes)}`, color: series[1] },
                { label: `Reserved ${bytes(r.disk.reservedBytes)}`, color: series[2] },
                { label: `Other ${bytes(Math.max(0, r.disk.totalBytes - r.disk.freeBytes - r.disk.yoursBytes - r.disk.friendsBytes))}`, color: c.outline },
                { label: `Free ${bytes(Math.max(0, r.disk.freeBytes - r.disk.reservedBytes))}`, color: c.surfaceContainerHighest },
              ]}
            />
          </Card>
        </>
      ) : null}

      <SectionHeader>Growth</SectionHeader>
      <Card>
        <Growth report={r} />
      </Card>
    </ScrollView>
  );
}

function LimitRow({ name, used, limit }: { name: string; used: number; limit: number | null }) {
  const { c } = useTheme();
  return (
    <View style={{ flexDirection: 'row', alignItems: 'center', gap: 10 }}>
      <Txt role="labelLarge" style={{ width: 80 }} numberOfLines={1}>
        {name}
      </Txt>
      <Txt style={{ width: 64, textAlign: 'right' }}>{bytes(used)}</Txt>
      <View style={{ flex: 1 }}>{limit != null ? <Bar segments={[{ value: used, color: withAlpha(c.primary, 0.6) }]} total={Math.max(limit, 1)} height={6} /> : null}</View>
      <Txt role="bodySmall" color={c.onSurfaceVariant} style={{ width: 56, textAlign: 'right' }}>
        {limit != null ? bytes(limit) : ''}
      </Txt>
    </View>
  );
}

/** Stored bytes by month, a projection at the recent pace, and the limit. */
function Growth({ report }: { report: StorageReport }) {
  const { c } = useTheme();
  const [width, setWidth] = useState(0);
  const h = 170;
  const padL = 48;
  const padR = 8;
  const padT = 12;
  const padB = 20;
  const hist = report.growth;
  const ahead = 6;
  const back = Math.min(3, hist.length - 1);
  const pace = back > 0 ? (hist[hist.length - 1].bytes - hist[hist.length - 1 - back].bytes) / back : 0;
  const points = hist.length + ahead;
  const last = hist.length ? hist[hist.length - 1].bytes : 0;
  const yMax = Math.max(report.limitBytes, last + pace * ahead, 1) * 1.05;
  const x = (i: number) => padL + (i * (width - padL - padR)) / Math.max(points - 1, 1);
  const y = (v: number) => padT + (1 - v / yMax) * (h - padT - padB);
  const monthName = (i: number) => {
    const [yy, mm] = (hist[hist.length - 1]?.month ?? '2026-01').split('-').map(Number);
    const d = new Date(Date.UTC(yy, mm - 1 + (i - (hist.length - 1)), 1));
    return d.toLocaleString(undefined, { month: 'short', timeZone: 'UTC' });
  };
  const step = width < 360 ? 3 : 2;
  return (
    <View onLayout={(e) => setWidth(e.nativeEvent.layout.width)} style={{ height: h }}>
      {width > 0 && hist.length ? (
        <Svg width={width} height={h}>
          {[0, yMax / 2, yMax].map((t) => (
            <Fragment key={t}>
              <Line x1={padL} x2={width - padR} y1={y(t)} y2={y(t)} stroke={c.outlineVariant} strokeWidth={1} />
              <SvgText x={padL - 6} y={y(t) + 4} fontSize={10} fill={c.onSurfaceVariant} textAnchor="end">
                {bytes(t)}
              </SvgText>
            </Fragment>
          ))}
          {report.limitBytes > 0 ? (
            <>
              <Line x1={padL} x2={width - padR} y1={y(report.limitBytes)} y2={y(report.limitBytes)} stroke={c.onSurfaceVariant} strokeWidth={1} strokeDasharray="2 3" />
              <SvgText x={width - padR} y={y(report.limitBytes) - 4} fontSize={10} fill={c.onSurfaceVariant} textAnchor="end">
                {bytes(report.limitBytes)}
              </SvgText>
            </>
          ) : null}
          <Polygon
            points={`${x(0)},${y(0)} ${hist.map((m, i) => `${x(i)},${y(m.bytes)}`).join(' ')} ${x(hist.length - 1)},${y(0)}`}
            fill={withAlpha(c.primary, 0.15)}
          />
          <Polyline points={hist.map((m, i) => `${x(i)},${y(m.bytes)}`).join(' ')} fill="none" stroke={c.primary} strokeWidth={2} />
          {pace > 0 ? (
            <Polyline
              points={Array.from({ length: ahead + 1 }, (_, i) => `${x(hist.length - 1 + i)},${y(last + pace * i)}`).join(' ')}
              fill="none"
              stroke={c.primary}
              strokeWidth={1.5}
              strokeDasharray="4 4"
            />
          ) : null}
          <Circle cx={x(hist.length - 1)} cy={y(last)} r={4} fill={c.primary} />
          {Array.from({ length: points }, (_, i) => i)
            .filter((i) => i % step === 0)
            .map((i) => (
              <SvgText key={i} x={x(i)} y={h - 4} fontSize={10} fill={c.onSurfaceVariant} textAnchor="middle">
                {monthName(i)}
              </SvgText>
            ))}
        </Svg>
      ) : null}
    </View>
  );
}
