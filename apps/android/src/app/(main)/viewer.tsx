import { Image } from 'expo-image';
import { router, useLocalSearchParams } from 'expo-router';
import { StatusBar } from 'expo-status-bar';
import { memo, useEffect, useMemo, useState } from 'react';
import { Alert, FlatList, Modal, Pressable, StyleSheet, useWindowDimensions, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { errorText, type LibraryItem } from 'ursa-core';

import { fetchFile, openFile, saveFile, shareFile } from '@/core/files';
import { useUrsa } from '@/core/store';
import { useThumb } from '@/core/thumbs';
import { bytes, dateTime, isMedia } from '@/format';
import { useTheme } from '@/theme';
import { Icon, IconButton, Loading, Progress, Txt } from '@/ui/kit';

/** Full-screen photos, swiped through in library order. */
export default function Viewer() {
  const { id } = useLocalSearchParams<{ id: string }>();
  const { items, remove } = useUrsa();
  const { width } = useWindowDimensions();
  const insets = useSafeAreaInsets();
  const media = useMemo(() => items.filter((i) => isMedia(i.mime)), [items]);
  const start = Math.max(0, media.findIndex((i) => i.fileId === id));
  const [index, setIndex] = useState(start);
  const [chrome, setChrome] = useState(true);
  const [info, setInfo] = useState(false);
  const [busy, setBusy] = useState<number | null>(null);
  const current = media[index];

  if (!current) return null;

  async function act(run: (i: LibraryItem, p: (f: number) => void) => Promise<void>) {
    setBusy(0);
    try {
      await run(current, (f) => setBusy(f));
    } catch (e) {
      Alert.alert('Something went wrong', errorText(e));
    } finally {
      setBusy(null);
    }
  }

  return (
    <View style={{ flex: 1, backgroundColor: '#000000' }}>
      <StatusBar style="light" hidden={!chrome} />
      <FlatList
        data={media}
        horizontal
        pagingEnabled
        initialScrollIndex={start}
        getItemLayout={(_, i) => ({ length: width, offset: width * i, index: i })}
        keyExtractor={(i) => i.fileId}
        showsHorizontalScrollIndicator={false}
        windowSize={3}
        onMomentumScrollEnd={(e) => setIndex(Math.round(e.nativeEvent.contentOffset.x / width))}
        renderItem={({ item, index: i }) => <Slide item={item} width={width} active={Math.abs(i - index) <= 1} onTap={() => setChrome((v) => !v)} />}
      />

      {chrome ? (
        <>
          <View style={[styles.bar, { paddingTop: insets.top + 4 }]}>
            <IconButton icon="arrow-back" label="Back" color="#FFFFFF" onPress={() => router.back()} />
            <View style={{ flex: 1, marginLeft: 8 }}>
              <Txt role="titleMedium" color="#FFFFFF" numberOfLines={1}>
                {dateTime(current.createdMs)}
              </Txt>
            </View>
            <IconButton icon="info-outline" label="Details" color="#FFFFFF" onPress={() => setInfo(true)} />
          </View>
          <View style={[styles.actions, { paddingBottom: insets.bottom + 8 }]}>
            {busy !== null ? (
              <View style={{ flex: 1, paddingHorizontal: 24 }}>
                <Progress progress={busy > 0 ? busy : null} />
              </View>
            ) : (
              <>
                <Action icon="share" label="Share" onPress={() => act(shareFile)} />
                <Action icon="download" label="Save" onPress={() => act(saveFile)} />
                {current.mime.startsWith('video/') ? <Action icon="play-arrow" label="Play" onPress={() => act(openFile)} /> : null}
                <Action
                  icon="delete-outline"
                  label="Delete"
                  onPress={() =>
                    Alert.alert('Delete this photo?', 'It is removed from every device and node.', [
                      { text: 'Cancel', style: 'cancel' },
                      {
                        text: 'Delete',
                        style: 'destructive',
                        onPress: async () => {
                          try {
                            await remove(current.fileId);
                            if (media.length <= 1) router.back();
                            else setIndex((i) => Math.min(i, media.length - 2));
                          } catch (e) {
                            Alert.alert('Could not delete', errorText(e));
                          }
                        },
                      },
                    ])
                  }
                />
              </>
            )}
          </View>
        </>
      ) : null}
      <Details item={current} visible={info} onClose={() => setInfo(false)} />
    </View>
  );
}

const Slide = memo(function Slide({ item, width, active, onTap }: { item: LibraryItem; width: number; active: boolean; onTap: () => void }) {
  const thumb = useThumb(item.fileId, item.hasThumb);
  const [full, setFull] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  const video = item.mime.startsWith('video/');

  useEffect(() => {
    if (!active || video || full) return;
    let live = true;
    fetchFile(item)
      .then((u) => live && setFull(u))
      .catch(() => live && setFailed(true));
    return () => {
      live = false;
    };
  }, [active, video, full, item]);

  return (
    <Pressable onPress={onTap} style={{ width, height: '100%', justifyContent: 'center' }}>
      {thumb && !full ? <Image source={{ uri: thumb }} style={StyleSheet.absoluteFill} contentFit="contain" /> : null}
      {full ? <Image source={{ uri: full }} style={StyleSheet.absoluteFill} contentFit="contain" transition={120} /> : null}
      {!full && !video && !failed ? (
        <View style={styles.loading}>
          <Loading />
        </View>
      ) : null}
      {video ? (
        <View style={styles.loading} pointerEvents="none">
          <Icon name="play-circle-outline" size={72} color="#FFFFFF" />
        </View>
      ) : null}
    </Pressable>
  );
});

function Action({ icon, label, onPress }: { icon: string; label: string; onPress: () => void }) {
  return (
    <Pressable onPress={onPress} android_ripple={{ color: '#FFFFFF22', borderless: true, radius: 36 }} style={{ alignItems: 'center', gap: 4, minWidth: 72, paddingVertical: 4 }}>
      <Icon name={icon} color="#FFFFFF" />
      <Txt role="labelMedium" color="#FFFFFF">
        {label}
      </Txt>
    </Pressable>
  );
}

function Details({ item, visible, onClose }: { item: LibraryItem; visible: boolean; onClose: () => void }) {
  const { c } = useTheme();
  const insets = useSafeAreaInsets();
  const row = (icon: string, title: string, sub?: string) => (
    <View style={{ flexDirection: 'row', gap: 16, paddingVertical: 10 }}>
      <Icon name={icon} />
      <View style={{ flex: 1 }}>
        <Txt role="bodyLarge">{title}</Txt>
        {sub ? (
          <Txt role="bodyMedium" color={c.onSurfaceVariant}>
            {sub}
          </Txt>
        ) : null}
      </View>
    </View>
  );
  return (
    <Modal visible={visible} transparent animationType="slide" onRequestClose={onClose} statusBarTranslucent navigationBarTranslucent>
      <Pressable style={{ flex: 1 }} onPress={onClose} />
      <View style={{ backgroundColor: c.surfaceContainerLow, borderTopLeftRadius: 28, borderTopRightRadius: 28, padding: 24, paddingBottom: insets.bottom + 16 }}>
        <Txt role="titleLarge" style={{ marginBottom: 8 }}>
          Details
        </Txt>
        {row('calendar-today', dateTime(item.createdMs))}
        {row(item.mime.startsWith('video/') ? 'movie' : 'image', item.name, [bytes(item.size), item.width && item.height ? `${item.width} × ${item.height}` : null].filter(Boolean).join(' · '))}
        {row('cloud-done', 'Backed up', `Added ${dateTime(item.addedMs)} · encrypted and spread across your pool`)}
      </View>
    </Modal>
  );
}

const styles = StyleSheet.create({
  bar: { position: 'absolute', top: 0, left: 0, right: 0, flexDirection: 'row', alignItems: 'center', paddingHorizontal: 4, paddingBottom: 8, backgroundColor: '#00000066' },
  actions: { position: 'absolute', bottom: 0, left: 0, right: 0, flexDirection: 'row', justifyContent: 'space-around', alignItems: 'center', paddingTop: 12, backgroundColor: '#00000066', minHeight: 72 },
  loading: { position: 'absolute', top: 0, left: 0, right: 0, bottom: 0, alignItems: 'center', justifyContent: 'center' },
});
