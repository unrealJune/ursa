import { FlashList } from '@shopify/flash-list';
import { Image } from 'expo-image';
import { router } from 'expo-router';
import { memo, useMemo } from 'react';
import { Pressable, RefreshControl, StyleSheet, useWindowDimensions, View } from 'react-native';
import type { LibraryItem } from 'ursa-core';

import { pickAndUpload } from '@/core/pick';
import { useUrsa } from '@/core/store';
import { useThumb } from '@/core/thumbs';
import { dayKey, dayLabel, isMedia } from '@/format';
import { useTheme } from '@/theme';
import { Button, Empty, Icon, IconButton, Notice, TopBar, Txt } from '@/ui/kit';
import { BackupButton, UploadStrip } from '@/ui/upload-status';

const GAP = 2;

type Row = { type: 'header'; key: string; label: string } | { type: 'row'; key: string; items: LibraryItem[] };

/** Day headers and rows of `columns` thumbnails, newest first. */
function layout(items: LibraryItem[], columns: number): Row[] {
  const rows: Row[] = [];
  let day = '';
  let row: LibraryItem[] = [];
  const flush = () => {
    if (row.length) rows.push({ type: 'row', key: `r-${row[0].fileId}`, items: row });
    row = [];
  };
  for (const item of items) {
    const k = dayKey(item.createdMs);
    if (k !== day) {
      flush();
      day = k;
      rows.push({ type: 'header', key: `h-${k}`, label: dayLabel(item.createdMs) });
    }
    row.push(item);
    if (row.length === columns) flush();
  }
  flush();
  return rows;
}

export default function Photos() {
  const { items, libLoading, libError, refreshLibrary, enqueue, prefs } = useUrsa();
  const { c } = useTheme();
  const { width } = useWindowDimensions();
  const columns = width >= 900 ? 7 : width >= 600 ? 5 : 4;
  const size = (width - GAP * (columns - 1)) / columns;
  const media = useMemo(() => items.filter((i) => isMedia(i.mime)), [items]);
  const rows = useMemo(() => layout(media, columns), [media, columns]);

  return (
    <View style={{ flex: 1, backgroundColor: c.surface }}>
      <TopBar
        title="Photos"
        actions={
          <>
            <IconButton icon="add-photo-alternate" label="Upload photos" onPress={() => pickAndUpload(enqueue)} />
            <BackupButton />
          </>
        }
      />
      <UploadStrip />
      {libError ? (
        <View style={{ paddingHorizontal: 16 }}>
          <Notice tone="error" text={libError} action={<Button label="Try again" kind="text" onPress={refreshLibrary} style={{ alignSelf: 'flex-start' }} />} />
        </View>
      ) : null}
      <FlashList
        data={rows}
        keyExtractor={(r) => r.key}
        getItemType={(r) => r.type}
        refreshControl={<RefreshControl refreshing={libLoading} onRefresh={refreshLibrary} colors={[c.primary]} progressBackgroundColor={c.surfaceContainerHigh} />}
        renderItem={({ item: r }) =>
          r.type === 'header' ? (
            <Txt role="titleSmall" style={{ paddingHorizontal: 16, paddingTop: 20, paddingBottom: 10 }}>
              {r.label}
            </Txt>
          ) : (
            <View style={{ flexDirection: 'row', gap: GAP, marginBottom: GAP }}>
              {r.items.map((item) => (
                <Cell key={item.fileId} item={item} size={size} />
              ))}
            </View>
          )
        }
        ListEmptyComponent={
          libLoading ? null : (
            <Empty
              icon="photo-library"
              title="No photos yet"
              body={prefs.backup ? 'New photos on this phone are backed up while Ursa is open.' : 'Back up this phone’s photos, or pick some to upload.'}
              action={
                <View style={{ gap: 8, marginTop: 8 }}>
                  {!prefs.backup ? <Button label="Turn on backup" icon="cloud-upload" onPress={() => router.push('/backup')} /> : null}
                  <Button label="Upload photos" kind="tonal" icon="add-photo-alternate" onPress={() => pickAndUpload(enqueue)} />
                </View>
              }
            />
          )
        }
      />
    </View>
  );
}

const Cell = memo(function Cell({ item, size }: { item: LibraryItem; size: number }) {
  const { c } = useTheme();
  const uri = useThumb(item.fileId, item.hasThumb);
  const video = item.mime.startsWith('video/');
  return (
    <Pressable
      onPress={() => router.push({ pathname: '/viewer', params: { id: item.fileId } })}
      style={{ width: size, height: size, backgroundColor: c.surfaceContainerHigh }}
      accessibilityLabel={item.name}
    >
      {uri ? (
        <Image source={{ uri }} style={StyleSheet.absoluteFill} contentFit="cover" transition={150} recyclingKey={item.fileId} />
      ) : !item.hasThumb ? (
        <View style={[StyleSheet.absoluteFill, { alignItems: 'center', justifyContent: 'center' }]}>
          <Icon name={video ? 'movie' : 'image'} color={c.onSurfaceVariant} />
        </View>
      ) : null}
      {video ? (
        <View style={{ position: 'absolute', top: 6, right: 6 }}>
          <Icon name="play-circle-filled" size={20} color="#FFFFFF" />
        </View>
      ) : null}
    </Pressable>
  );
});
