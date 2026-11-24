import * as Haptics from 'expo-haptics';
import { useEffect, useMemo, useRef, useState } from 'react';
import { Animated, StyleSheet, View } from 'react-native';
import { GestureDetector, usePanGesture } from 'react-native-gesture-handler';

import type { Store } from '@/core/state';
import { monthLabel } from '@/format';
import { useTheme } from '@/theme';
import { Icon, Txt } from '@/ui/kit';

import { type Layout, timeAt, yearMarks } from './layout';

const THUMB = 48;
const MARGIN = 8;
/** Only a library longer than this many screens gets a scrubber. */
const MIN_SCREENS = 3;
const HIDE_AFTER_MS = 1500;

/**
 * The fast-scroll handle on the right edge: it shows while the grid scrolls, and dragging it
 * jumps through the whole library with the month under it in a bubble and the years marked
 * along the track.
 */
export function Scrubber({
  layout,
  viewport,
  scroll,
  onScrollTo,
}: {
  layout: Layout;
  viewport: number;
  scroll: Store<{ y: number }>;
  onScrollTo: (y: number) => void;
}) {
  const { c } = useTheme();
  const y = scroll.use((s) => s.y);
  const [dragging, setDragging] = useState(false);
  const [shown, setShown] = useState(false);
  const [opacity] = useState(() => new Animated.Value(0));
  const hideTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const dragFrom = useRef(0);
  const lastYear = useRef<number | null>(null);

  const maxScroll = Math.max(1, layout.height - viewport);
  const track = Math.max(1, viewport - THUMB - MARGIN * 2);
  const pos = Math.max(0, Math.min(track, (y / maxScroll) * track));
  const enough = layout.height > viewport * MIN_SCREENS;

  // Show while scrolling or dragging; fade out a moment after.
  const hideLater = () => {
    if (hideTimer.current) clearTimeout(hideTimer.current);
    hideTimer.current = setTimeout(() => setShown(false), HIDE_AFTER_MS);
  };
  useEffect(() => {
    const unsubscribe = scroll.subscribe(() => {
      setShown(true);
      hideLater();
    });
    return () => {
      unsubscribe();
      if (hideTimer.current) clearTimeout(hideTimer.current);
    };
  }, [scroll]);
  useEffect(() => {
    Animated.timing(opacity, { toValue: shown ? 1 : 0, duration: shown ? 120 : 300, useNativeDriver: true }).start();
  }, [shown, opacity]);

  const marks = useMemo(() => {
    const out: { year: number; at: number }[] = [];
    for (const m of yearMarks(layout)) {
      const at = (Math.min(m.top, maxScroll) / maxScroll) * track + THUMB / 2;
      if (out.length && at - out[out.length - 1].at < 22) continue;
      out.push({ year: m.year, at });
    }
    return out;
  }, [layout, maxScroll, track]);

  const pan = usePanGesture({
    runOnJS: true,
    minDistance: 0,
    hitSlop: { left: 24, top: 8, bottom: 8 },
    onActivate: () => {
      if (hideTimer.current) clearTimeout(hideTimer.current);
      dragFrom.current = pos;
      lastYear.current = null;
      setDragging(true);
    },
    onUpdate: (e) => {
      const p = Math.max(0, Math.min(track, dragFrom.current + e.translationY));
      const target = (p / track) * maxScroll;
      onScrollTo(target);
      // A tick each time the drag crosses into another year.
      const t = timeAt(layout, target);
      const year = t === null ? null : new Date(t).getFullYear();
      if (year !== null && lastYear.current !== null && year !== lastYear.current) {
        Haptics.performAndroidHapticsAsync(Haptics.AndroidHaptics.Segment_Tick).catch(() => {});
      }
      lastYear.current = year;
    },
    onFinalize: () => {
      setDragging(false);
      hideLater();
    },
  });

  if (!enough) return null;
  const at = timeAt(layout, y);

  return (
    <Animated.View style={[StyleSheet.absoluteFill, { opacity }]} pointerEvents={shown ? 'box-none' : 'none'}>
      {dragging
        ? marks.map((m) => (
            <View key={m.year} pointerEvents="none" style={[styles.mark, { top: MARGIN + m.at - 10, backgroundColor: c.surfaceContainerHigh }]}>
              <Txt role="labelMedium" color={c.onSurfaceVariant}>
                {String(m.year)}
              </Txt>
            </View>
          ))
        : null}
      {dragging && at !== null ? (
        <View pointerEvents="none" style={[styles.bubble, { top: MARGIN + pos + THUMB / 2 - 24, backgroundColor: c.primaryContainer }]}>
          <Txt role="titleMedium" color={c.onPrimaryContainer}>
            {monthLabel(at)}
          </Txt>
        </View>
      ) : null}
      <GestureDetector gesture={pan}>
        <View
          accessibilityRole="adjustable"
          accessibilityLabel="Scroll through the library by date"
          style={[styles.thumb, { top: MARGIN + pos, backgroundColor: dragging ? c.primary : c.surfaceContainerHighest }]}
        >
          <Icon name="unfold-more" size={22} color={dragging ? c.onPrimary : c.onSurfaceVariant} />
        </View>
      </GestureDetector>
    </Animated.View>
  );
}

const styles = StyleSheet.create({
  thumb: {
    position: 'absolute',
    right: 0,
    width: 36,
    height: THUMB,
    borderTopLeftRadius: THUMB / 2,
    borderBottomLeftRadius: THUMB / 2,
    alignItems: 'center',
    justifyContent: 'center',
    elevation: 3,
  },
  bubble: { position: 'absolute', right: 52, height: 48, paddingHorizontal: 20, borderRadius: 24, justifyContent: 'center', elevation: 3 },
  mark: { position: 'absolute', right: 44, height: 20, paddingHorizontal: 8, borderRadius: 10, justifyContent: 'center' },
});
