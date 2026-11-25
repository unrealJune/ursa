import { type ReactNode } from 'react';
import { type StyleProp, View, type ViewStyle } from 'react-native';

import { useTheme } from '@/theme';

import { Txt } from './kit';

/** A rounded block on the container colour, for the Pool and Usage screens. */
export function Card({ children, style }: { children: ReactNode; style?: StyleProp<ViewStyle> }) {
  const { c } = useTheme();
  return <View style={[{ marginHorizontal: 16, padding: 20, borderRadius: 24, backgroundColor: c.surfaceContainer }, style]}>{children}</View>;
}

type Segment = { value: number; color: string };

/**
 * A horizontal bar of segments, each a share of `total`; optionally a mark at `tick` and a band
 * over a range (the room a switch takes past what is stored).
 */
export function Bar({
  segments,
  total,
  height = 12,
  tick,
  band,
}: {
  segments: Segment[];
  total: number;
  height?: number;
  tick?: number;
  band?: { from: number; to: number; color: string };
}) {
  const { c } = useTheme();
  const pct = (v: number) => `${Math.max(0, Math.min(100, (v / Math.max(total, 1)) * 100))}%` as const;
  // Each segment's width, clipped so together they never pass the total.
  const widths = segments.reduce<number[]>((acc, s) => {
    const used = acc.reduce((a, b) => a + b, 0);
    return [...acc, Math.max(0, Math.min(s.value, total - used))];
  }, []);
  return (
    <View style={{ height, borderRadius: height / 2, backgroundColor: c.surfaceContainerHighest, overflow: 'visible' }}>
      <View style={{ flexDirection: 'row', height, borderRadius: height / 2, overflow: 'hidden' }}>
        {segments.map((s, i) => (
          <View key={i} style={{ width: pct(widths[i]), backgroundColor: s.color }} />
        ))}
      </View>
      {band ? (
        <View
          style={{
            position: 'absolute',
            left: pct(band.from),
            width: pct(Math.max(band.to - band.from, 0)),
            minWidth: 3,
            top: -3,
            bottom: -3,
            borderRadius: 3,
            backgroundColor: band.color,
            opacity: 0.8,
          }}
        />
      ) : null}
      {tick != null ? <View style={{ position: 'absolute', left: pct(tick), top: -3, bottom: -3, width: 2, backgroundColor: c.onSurface }} /> : null}
    </View>
  );
}

export function Legend({ items }: { items: { label: string; color: string }[] }) {
  const { c } = useTheme();
  return (
    <View style={{ flexDirection: 'row', flexWrap: 'wrap', columnGap: 14, rowGap: 4 }}>
      {items.map((it) => (
        <View key={it.label} style={{ flexDirection: 'row', alignItems: 'center', gap: 6 }}>
          <View style={{ width: 10, height: 10, borderRadius: 3, backgroundColor: it.color }} />
          <Txt role="bodySmall" color={c.onSurfaceVariant}>
            {it.label}
          </Txt>
        </View>
      ))}
    </View>
  );
}

/** Label on the left, value on the right. */
export function Row({ label, value, strong }: { label: string; value: string; strong?: boolean }) {
  const { c } = useTheme();
  return (
    <View style={{ flexDirection: 'row', paddingVertical: 3 }}>
      <Txt color={c.onSurfaceVariant} style={{ flex: 1 }}>
        {label}
      </Txt>
      <Txt role={strong ? 'labelLarge' : 'bodyMedium'}>{value}</Txt>
    </View>
  );
}
