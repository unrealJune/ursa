import { FlashList, type FlashListRef } from '@shopify/flash-list';
import * as Haptics from 'expo-haptics';
import { Image } from 'expo-image';
import { createContext, memo, type ReactElement, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { Pressable, type ScrollViewProps, StyleSheet, View } from 'react-native';
import {
  GestureDetector,
  RefreshControl,
  ScrollView as GestureScrollView,
  useCompetingGestures,
  usePanGesture,
  usePinchGesture,
  type PinchGesture,
} from 'react-native-gesture-handler';
import type { LibraryItem } from 'ursa-core';

import { createStore } from '@/core/state';
import { useThumb } from '@/core/thumbs';
import { useTheme } from '@/theme';
import { Icon, Txt, withAlpha } from '@/ui/kit';

import { BOTTOM_PADDING, clampZoom, type Entry, GAP, HEADER_HEIGHT, itemAt, layout as lay, rowTop } from './layout';
import { Scrubber } from './scrubber';

/** How close to the top or bottom edge a drag-select starts scrolling, and how fast at most. */
const EDGE = 72;
const MAX_SPEED = 22;
/** How far a pinch goes before the grid steps one zoom level. */
const PINCH_STEP = 1.28;
/** Hold this long before a drag selects instead of scrolling. */
const LONG_PRESS_MS = 280;

export type Selection = ReadonlySet<string>;

type Props = {
  items: LibraryItem[];
  width: number;
  zoom: number;
  onZoom: (zoom: number) => void;
  selected: Selection;
  onSelect: (next: Selection) => void;
  onOpen: (item: LibraryItem) => void;
  refreshing: boolean;
  onRefresh: () => void;
  empty?: ReactElement | null;
};

/**
 * The photo grid, Google Photos style:
 * - pinch to zoom between a few large photos and a dense month-by-month overview, with the photo
 *   under the fingers kept where it was;
 * - hold a photo to select it, and keep dragging to select (or, starting on a selected photo,
 *   deselect) everything between; near the top or bottom edge the grid scrolls on its own;
 * - while selecting, a tap toggles a photo, and a tap on a date toggles that whole day or month;
 * - a scrubber on the right edge jumps through the library by date.
 */
export function Grid({ items, width, zoom, onZoom, selected, onSelect, onOpen, refreshing, onRefresh, empty }: Props) {
  const { c } = useTheme();
  const l = useMemo(() => lay(items, width, zoom), [items, width, zoom]);
  const list = useRef<FlashListRef<Entry>>(null);
  const scroll = useMemo(() => createStore({ y: 0 }), []);
  const [viewport, setViewport] = useState(0);
  const [pinching, setPinching] = useState(false);
  const selecting = selected.size > 0;

  // The latest values, for gesture callbacks and timers that outlive a render.
  const live = useRef({ l, items, selected, onSelect, zoom, onZoom, viewport });
  useLayoutEffect(() => {
    live.current = { l, items, selected, onSelect, zoom, onZoom, viewport };
  });

  const scrollTo = (y: number) => {
    const max = Math.max(0, live.current.l.height - live.current.viewport);
    const clamped = Math.max(0, Math.min(max, y));
    list.current?.scrollToOffset({ offset: clamped, animated: false });
    scroll.set({ y: clamped });
  };

  // ---- Zoom, kept anchored on the photo under the fingers ----

  const pinch = useRef({ base: 1, focalY: 0 });
  const anchor = useRef<{ index: number; screenY: number } | null>(null);

  const stepZoom = (dir: 1 | -1, focalX: number, focalY: number) => {
    const { zoom: z, l: cur } = live.current;
    const next = clampZoom(z + dir);
    if (next === z) return;
    const index = itemAt(cur, focalX, scroll.get().y + focalY);
    anchor.current = index === null ? null : { index, screenY: focalY };
    Haptics.performAndroidHapticsAsync(Haptics.AndroidHaptics.Segment_Tick).catch(() => {});
    live.current.onZoom(next);
  };

  useLayoutEffect(() => {
    const a = anchor.current;
    if (!a) return;
    anchor.current = null;
    const target = rowTop(l, a.index) + l.size / 2 - a.screenY;
    // After the list has taken the new rows.
    requestAnimationFrame(() => scrollTo(target));
    // eslint-disable-next-line react-hooks/exhaustive-deps -- runs when the layout changes
  }, [l]);

  const pinchGesture = usePinchGesture({
    runOnJS: true,
    onActivate: (e) => {
      pinch.current = { base: e.scale, focalY: e.focalY };
      setPinching(true);
    },
    onUpdate: (e) => {
      const ratio = e.scale / pinch.current.base;
      if (ratio > PINCH_STEP) {
        pinch.current.base = e.scale;
        stepZoom(-1, e.focalX, e.focalY);
      } else if (ratio < 1 / PINCH_STEP) {
        pinch.current.base = e.scale;
        stepZoom(1, e.focalX, e.focalY);
      }
    },
    onFinalize: () => setPinching(false),
  });

  // ---- Hold, then drag, to select ----

  const drag = useRef<{ anchor: number; last: number; base: Set<string>; adding: boolean; x: number; y: number } | null>(null);
  const autoScroll = useRef<number | null>(null);
  const lastDragEnd = useRef(0);

  const selectTo = (index: number) => {
    const d = drag.current;
    if (!d || index === d.last) return;
    d.last = index;
    const { items: all, onSelect: set } = live.current;
    const next = new Set(d.base);
    const [from, to] = d.anchor <= index ? [d.anchor, index] : [index, d.anchor];
    for (let i = from; i <= to; i++) {
      if (d.adding) next.add(all[i].fileId);
      else next.delete(all[i].fileId);
    }
    set(next);
  };

  const pointAt = () => {
    const d = drag.current;
    if (!d) return;
    const index = itemAt(live.current.l, d.x, scroll.get().y + d.y);
    if (index !== null) selectTo(index);
  };

  const stopAutoScroll = () => {
    if (autoScroll.current !== null) cancelAnimationFrame(autoScroll.current);
    autoScroll.current = null;
  };

  const runAutoScroll = () => {
    const d = drag.current;
    const h = live.current.viewport;
    if (!d) return stopAutoScroll();
    const speed = d.y < EDGE ? -MAX_SPEED * (1 - Math.max(0, d.y) / EDGE) : d.y > h - EDGE ? MAX_SPEED * (1 - Math.max(0, h - d.y) / EDGE) : 0;
    if (speed === 0) return stopAutoScroll();
    scrollTo(scroll.get().y + speed);
    pointAt();
    autoScroll.current = requestAnimationFrame(runAutoScroll);
  };

  const selectGesture = usePanGesture({
    runOnJS: true,
    activateAfterLongPress: LONG_PRESS_MS,
    maxPointers: 1,
    onActivate: (e) => {
      const { l: cur, items: all, selected: sel } = live.current;
      const index = itemAt(cur, e.x, scroll.get().y + e.y);
      if (index === null || !all[index]) return;
      const id = all[index].fileId;
      // Starting on a photo already selected deselects; anywhere else adds.
      const adding = !sel.has(id);
      drag.current = { anchor: index, last: -1, base: new Set(sel), adding, x: e.x, y: e.y };
      Haptics.performAndroidHapticsAsync(Haptics.AndroidHaptics.Long_Press).catch(() => {});
      selectTo(index);
    },
    onUpdate: (e) => {
      const d = drag.current;
      if (!d) return;
      d.x = e.x;
      d.y = e.y;
      pointAt();
      if (autoScroll.current === null) runAutoScroll();
    },
    onFinalize: () => {
      if (drag.current) lastDragEnd.current = Date.now();
      drag.current = null;
      stopAutoScroll();
    },
  });

  useEffect(() => stopAutoScroll, []);

  const gesture = useCompetingGestures(pinchGesture, selectGesture);

  // ---- Taps ----

  const tap = (item: LibraryItem) => {
    // The release that ends a hold-and-drag is not a tap.
    if (Date.now() - lastDragEnd.current < 350) return;
    const { selected: sel, onSelect: set } = live.current;
    if (sel.size === 0) return onOpen(item);
    const next = new Set(sel);
    if (next.has(item.fileId)) next.delete(item.fileId);
    else next.add(item.fileId);
    set(next);
  };

  const toggleGroup = (start: number, end: number) => {
    const { items: all, selected: sel, onSelect: set } = live.current;
    const ids = all.slice(start, end).map((i) => i.fileId);
    const next = new Set(sel);
    const every = ids.every((id) => next.has(id));
    for (const id of ids) {
      if (every) next.delete(id);
      else next.add(id);
    }
    set(next);
  };

  return (
    <View style={{ flex: 1 }} onLayout={(e) => setViewport(e.nativeEvent.layout.height)}>
      <ScrollRelations.Provider value={pinchGesture}>
        <GestureDetector gesture={gesture}>
          <View style={{ flex: 1 }} collapsable={false}>
            <FlashList
              ref={list}
              data={l.entries}
              extraData={selected}
              keyExtractor={(e) => e.key}
              getItemType={(e) => e.type}
              renderScrollComponent={Scroll}
              scrollEnabled={!pinching}
              onScroll={(e) => scroll.set({ y: e.nativeEvent.contentOffset.y })}
              scrollEventThrottle={16}
              showsVerticalScrollIndicator={false}
              contentContainerStyle={{ paddingBottom: BOTTOM_PADDING }}
              refreshControl={<RefreshControl refreshing={refreshing} onRefresh={onRefresh} colors={[c.primary]} progressBackgroundColor={c.surfaceContainerHigh} />}
              renderItem={({ item: e }) =>
                e.type === 'header' ? (
                  <GroupHeader
                    label={e.label}
                    selecting={selecting}
                    state={groupState(items, e.start, e.end, selected)}
                    onPress={() => toggleGroup(e.start, e.end)}
                  />
                ) : (
                  <View style={{ flexDirection: 'row', gap: GAP, marginBottom: GAP }}>
                    {e.items.map((item) => (
                      <Cell key={item.fileId} item={item} size={l.size} small={l.columns >= 6} selecting={selecting} selected={selected.has(item.fileId)} onPress={tap} />
                    ))}
                  </View>
                )
              }
              ListEmptyComponent={empty}
            />
          </View>
        </GestureDetector>
      </ScrollRelations.Provider>
      {viewport > 0 && items.length > 0 ? <Scrubber layout={l} viewport={viewport} scroll={scroll} onScrollTo={scrollTo} /> : null}
    </View>
  );
}

/**
 * The list's scroll view, from Gesture Handler so it takes part in gesture arbitration: a held
 * drag-select cancels it, and it scrolls alongside a pinch (which then turns scrolling off).
 */
const ScrollRelations = createContext<PinchGesture | undefined>(undefined);

function Scroll(props: ScrollViewProps & { ref?: React.Ref<never> }) {
  const pinch = useContext(ScrollRelations);
  return <GestureScrollView {...props} simultaneousWith={pinch} />;
}

function groupState(items: LibraryItem[], start: number, end: number, selected: Selection): 'none' | 'some' | 'all' {
  if (selected.size === 0) return 'none';
  let n = 0;
  for (let i = start; i < end; i++) if (selected.has(items[i].fileId)) n += 1;
  return n === 0 ? 'none' : n === end - start ? 'all' : 'some';
}

function GroupHeader({ label, selecting, state, onPress }: { label: string; selecting: boolean; state: 'none' | 'some' | 'all'; onPress: () => void }) {
  const { c } = useTheme();
  return (
    <Pressable
      onPress={selecting ? onPress : undefined}
      disabled={!selecting}
      accessibilityRole={selecting ? 'checkbox' : 'header'}
      accessibilityState={selecting ? { checked: state === 'all' ? true : state === 'some' ? 'mixed' : false } : undefined}
      style={{ height: HEADER_HEIGHT, flexDirection: 'row', alignItems: 'center', paddingHorizontal: 16, gap: 12 }}
    >
      {selecting ? (
        <Icon
          name={state === 'all' ? 'check-circle' : state === 'some' ? 'remove-circle-outline' : 'radio-button-unchecked'}
          size={22}
          color={state === 'none' ? c.onSurfaceVariant : c.primary}
        />
      ) : null}
      <Txt role="titleSmall">{label}</Txt>
    </Pressable>
  );
}

const Cell = memo(function Cell({
  item,
  size,
  small,
  selecting,
  selected,
  onPress,
}: {
  item: LibraryItem;
  size: number;
  small: boolean;
  selecting: boolean;
  selected: boolean;
  onPress: (item: LibraryItem) => void;
}) {
  const { c } = useTheme();
  const uri = useThumb(item.fileId, item.hasThumb);
  const video = item.mime.startsWith('video/');
  // A selected photo shrinks into its tile, as in Google Photos.
  const inset = selected ? Math.min(14, size * 0.1) : 0;
  return (
    <Pressable
      onPress={() => onPress(item)}
      style={{ width: size, height: size, backgroundColor: selected ? c.secondaryContainer : c.surfaceContainerHigh }}
      accessibilityLabel={item.name}
      accessibilityState={selecting ? { selected } : undefined}
    >
      <View style={[StyleSheet.absoluteFill, { margin: inset, borderRadius: selected ? 12 : 0, overflow: 'hidden', backgroundColor: c.surfaceContainerHigh }]}>
        {uri ? (
          <Image source={{ uri }} style={StyleSheet.absoluteFill} contentFit="cover" transition={small ? 0 : 150} recyclingKey={item.fileId} />
        ) : !item.hasThumb ? (
          <View style={[StyleSheet.absoluteFill, { alignItems: 'center', justifyContent: 'center' }]}>
            <Icon name={video ? 'movie' : 'image'} size={small ? 16 : 24} color={c.onSurfaceVariant} />
          </View>
        ) : null}
      </View>
      {video && !small ? (
        <View style={{ position: 'absolute', top: 6, right: 6 }}>
          <Icon name="play-circle-filled" size={20} color="#FFFFFF" />
        </View>
      ) : null}
      {selecting && !small ? (
        <View style={{ position: 'absolute', top: 6, left: 6 }}>
          {selected ? (
            <View style={{ borderRadius: 12, backgroundColor: c.surface }}>
              <Icon name="check-circle" size={24} color={c.primary} />
            </View>
          ) : (
            <Icon name="radio-button-unchecked" size={24} color={withAlpha('#FFFFFF', 0.9)} />
          )}
        </View>
      ) : null}
      {selecting && small && selected ? (
        <View style={{ position: 'absolute', top: 2, left: 2, borderRadius: 8, backgroundColor: c.surface }}>
          <Icon name="check-circle" size={16} color={c.primary} />
        </View>
      ) : null}
    </Pressable>
  );
});
