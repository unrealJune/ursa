import { router, useFocusEffect } from 'expo-router';
import { useCallback, useMemo, useState } from 'react';
import { Alert, BackHandler, useWindowDimensions, View } from 'react-native';
import { errorText } from 'ursa-core';

import { shareFile } from '@/core/files';
import { loadLibrary, removeMany, saveMany, useLibrary } from '@/core/library';
import { pickAndUpload } from '@/core/pick';
import { setPrefs, useSession } from '@/core/session';
import { isMedia } from '@/format';
import { useTheme } from '@/theme';
import { Grid, type Selection } from '@/ui/gallery/grid';
import { Button, Empty, IconButton, Notice, TopBar } from '@/ui/kit';
import { BackupButton, BulkStrip, UploadStrip } from '@/ui/upload-status';

const NONE: Selection = new Set();

export default function Photos() {
  const items = useLibrary((s) => s.items);
  const pulling = useLibrary((s) => s.pulling);
  const loaded = useLibrary((s) => s.loaded);
  const error = useLibrary((s) => s.error);
  const busy = useLibrary((s) => s.bulk !== null);
  const backup = useSession((s) => s.prefs.backup);
  const zoom = useSession((s) => s.prefs.gridZoom);
  const { c } = useTheme();
  const { width } = useWindowDimensions();
  const media = useMemo(() => items.filter((i) => isMedia(i.mime)), [items]);
  const [picked, setPicked] = useState<Selection>(NONE);

  // Only what is still in the library (a delete, or another device's, may have taken some).
  const selected = useMemo(() => {
    if (picked.size === 0) return NONE;
    const ids = new Set(media.map((i) => i.fileId));
    const kept = [...picked].filter((id) => ids.has(id));
    return kept.length === picked.size ? picked : new Set(kept);
  }, [picked, media]);
  const selecting = selected.size > 0;
  const chosen = () => media.filter((i) => selected.has(i.fileId));
  const clear = () => setPicked(NONE);

  // Back leaves selection before it leaves the screen.
  useFocusEffect(
    useCallback(() => {
      if (!selecting) return;
      const sub = BackHandler.addEventListener('hardwareBackPress', () => {
        setPicked(NONE);
        return true;
      });
      return () => sub.remove();
    }, [selecting]),
  );

  const refresh = () => loadLibrary({ fromNodes: true, reason: 'pull' });

  const save = async () => {
    const list = chosen();
    clear();
    await saveMany(list);
  };

  const share = async () => {
    const [one] = chosen();
    if (!one) return;
    try {
      await shareFile(one);
      clear();
    } catch (e) {
      Alert.alert('Could not share', errorText(e));
    }
  };

  const del = () => {
    const list = chosen();
    const n = list.length;
    Alert.alert(n === 1 ? 'Delete this item?' : `Delete ${n} items?`, `${n === 1 ? 'It is' : 'They are'} removed from every device and node. This cannot be undone.`, [
      { text: 'Cancel', style: 'cancel' },
      {
        text: 'Delete',
        style: 'destructive',
        onPress: async () => {
          clear();
          await removeMany(list);
        },
      },
    ]);
  };

  return (
    <View style={{ flex: 1, backgroundColor: c.surface }}>
      {selecting ? (
        <TopBar
          title={`${selected.size} selected`}
          background={c.surfaceContainer}
          leading={<IconButton icon="close" label="Clear selection" onPress={clear} />}
          actions={
            <>
              {selected.size < media.length ? <IconButton icon="select-all" label="Select all" onPress={() => setPicked(new Set(media.map((i) => i.fileId)))} /> : null}
              {selected.size === 1 ? <IconButton icon="share" label="Share" onPress={share} /> : null}
              {!busy ? <IconButton icon="download" label="Save to this phone" onPress={save} /> : null}
              {!busy ? <IconButton icon="delete-outline" label="Delete" onPress={del} /> : null}
            </>
          }
        />
      ) : (
        <TopBar
          title="Photos"
          actions={
            <>
              <IconButton icon="add-photo-alternate" label="Upload photos" onPress={pickAndUpload} />
              <BackupButton />
            </>
          }
        />
      )}
      <UploadStrip />
      <BulkStrip />
      {error ? (
        <View style={{ paddingHorizontal: 16 }}>
          <Notice
            tone="error"
            text={`Could not reach your nodes, so this may not show the latest from your other devices. ${error}`}
            action={<Button label="Try again" kind="text" onPress={refresh} style={{ alignSelf: 'flex-start' }} />}
          />
        </View>
      ) : null}
      <Grid
        items={media}
        width={width}
        zoom={zoom}
        onZoom={(z) => setPrefs({ gridZoom: z })}
        selected={selected}
        onSelect={setPicked}
        onOpen={(item) => router.push({ pathname: '/viewer', params: { id: item.fileId } })}
        refreshing={pulling}
        onRefresh={refresh}
        empty={
          !loaded ? null : (
            <Empty
              icon="photo-library"
              title="No photos yet"
              body={backup ? 'New photos on this phone are backed up while Ursa is open.' : 'Back up this phone’s photos, or pick some to upload.'}
              action={
                <View style={{ gap: 8, marginTop: 8 }}>
                  {!backup ? <Button label="Turn on backup" icon="cloud-upload" onPress={() => router.push('/backup')} /> : null}
                  <Button label="Upload photos" kind="tonal" icon="add-photo-alternate" onPress={pickAndUpload} />
                </View>
              }
            />
          )
        }
      />
    </View>
  );
}
