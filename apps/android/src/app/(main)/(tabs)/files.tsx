import { FlashList } from '@shopify/flash-list';
import { useMemo, useState } from 'react';
import { Alert, Pressable, RefreshControl, View } from 'react-native';
import { errorText, type LibraryItem } from 'ursa-core';

import { openFile, saveFile, shareFile } from '@/core/files';
import { pickFiles } from '@/core/pick';
import { useUrsa } from '@/core/store';
import { bytes, dayLabel, fileIcon, isMedia } from '@/format';
import { useTheme } from '@/theme';
import { Button, Empty, Fab, Icon, IconButton, Notice, Progress, Sheet, TopBar, Txt, withAlpha } from '@/ui/kit';
import { BackupButton, UploadStrip } from '@/ui/upload-status';

type Sort = 'added' | 'name' | 'size';

export default function Files() {
  const { items, libLoading, libError, refreshLibrary, enqueue, remove } = useUrsa();
  const { c } = useTheme();
  const [all, setAll] = useState(false);
  const [sort, setSort] = useState<Sort>('added');
  const [menu, setMenu] = useState<LibraryItem | null>(null);
  const [sortMenu, setSortMenu] = useState(false);
  const [working, setWorking] = useState<{ id: string; progress: number } | null>(null);
  const [error, setError] = useState<string | null>(null);

  const files = useMemo(() => {
    const list = all ? [...items] : items.filter((i) => !isMedia(i.mime));
    if (sort === 'name') list.sort((a, b) => a.name.localeCompare(b.name));
    else if (sort === 'size') list.sort((a, b) => b.size - a.size);
    else list.sort((a, b) => b.addedMs - a.addedMs);
    return list;
  }, [items, all, sort]);

  async function act(item: LibraryItem, run: (i: LibraryItem, p: (f: number) => void) => Promise<void>) {
    setError(null);
    setWorking({ id: item.fileId, progress: 0 });
    try {
      await run(item, (f) => setWorking({ id: item.fileId, progress: f }));
    } catch (e) {
      setError(`${item.name}: ${errorText(e)}`);
    } finally {
      setWorking(null);
    }
  }

  return (
    <View style={{ flex: 1, backgroundColor: c.surface }}>
      <TopBar title="Files" actions={<><IconButton icon="sort" label="Sort" onPress={() => setSortMenu(true)} /><BackupButton /></>} />
      <View style={{ flexDirection: 'row', gap: 8, paddingHorizontal: 16, paddingBottom: 8 }}>
        <Chip label="Documents" selected={!all} onPress={() => setAll(false)} />
        <Chip label="Everything" selected={all} onPress={() => setAll(true)} />
      </View>
      <UploadStrip />
      {libError || error ? (
        <View style={{ paddingHorizontal: 16 }}>
          <Notice tone="error" text={(error ?? libError)!} />
        </View>
      ) : null}
      <FlashList
        data={files}
        keyExtractor={(i) => i.fileId}
        refreshControl={<RefreshControl refreshing={libLoading} onRefresh={refreshLibrary} colors={[c.primary]} progressBackgroundColor={c.surfaceContainerHigh} />}
        contentContainerStyle={{ paddingBottom: 96 }}
        renderItem={({ item }) => (
          <FileRow
            item={item}
            progress={working?.id === item.fileId ? working.progress : null}
            onPress={() => act(item, openFile)}
            onMore={() => setMenu(item)}
          />
        )}
        ListEmptyComponent={
          libLoading ? null : (
            <Empty
              icon="folder-open"
              title="No files yet"
              body="Documents, archives, anything: encrypted on this phone and kept across your pool."
              action={<Button label="Upload files" icon="upload-file" onPress={() => pickFiles(enqueue)} style={{ marginTop: 8 }} />}
            />
          )
        }
      />
      <Fab icon="upload-file" label="Upload" onPress={() => pickFiles(enqueue)} />

      <Sheet
        visible={menu !== null}
        title={menu?.name}
        onClose={() => setMenu(null)}
        actions={
          menu
            ? [
                { icon: 'open-in-new', label: 'Open', onPress: () => act(menu, openFile) },
                { icon: 'share', label: 'Share', onPress: () => act(menu, shareFile) },
                { icon: 'download', label: 'Save to this phone', onPress: () => act(menu, saveFile) },
                {
                  icon: 'delete-outline',
                  label: 'Delete',
                  danger: true,
                  onPress: () =>
                    Alert.alert('Delete this file?', `${menu.name} is removed from every device and node.`, [
                      { text: 'Cancel', style: 'cancel' },
                      { text: 'Delete', style: 'destructive', onPress: () => remove(menu.fileId).catch((e) => setError(errorText(e))) },
                    ]),
                },
              ]
            : []
        }
      />
      <Sheet
        visible={sortMenu}
        title="Sort by"
        onClose={() => setSortMenu(false)}
        actions={(
          [
            ['added', 'Date added', 'schedule'],
            ['name', 'Name', 'sort-by-alpha'],
            ['size', 'Size', 'data-usage'],
          ] as const
        ).map(([key, label, icon]) => ({ icon: sort === key ? 'check' : icon, label, onPress: () => setSort(key) }))}
      />
    </View>
  );
}

function FileRow({ item, progress, onPress, onMore }: { item: LibraryItem; progress: number | null; onPress: () => void; onMore: () => void }) {
  const { c } = useTheme();
  return (
    <Pressable
      onPress={onPress}
      onLongPress={onMore}
      android_ripple={{ color: withAlpha(c.onSurface, 0.08) }}
      style={{ flexDirection: 'row', alignItems: 'center', paddingLeft: 16, paddingRight: 4, paddingVertical: 10, gap: 16 }}
    >
      <View style={{ width: 40, height: 40, borderRadius: 12, backgroundColor: c.secondaryContainer, alignItems: 'center', justifyContent: 'center' }}>
        <Icon name={fileIcon(item.mime, item.name)} color={c.onSecondaryContainer} size={22} />
      </View>
      <View style={{ flex: 1, gap: 2 }}>
        <Txt role="bodyLarge" numberOfLines={1}>
          {item.name}
        </Txt>
        {progress !== null ? (
          <Progress progress={progress > 0 ? progress : null} />
        ) : (
          <Txt role="bodyMedium" color={c.onSurfaceVariant} numberOfLines={1}>
            {bytes(item.size)} · {dayLabel(item.addedMs)}
          </Txt>
        )}
      </View>
      <IconButton icon="more-vert" label={`More for ${item.name}`} onPress={onMore} />
    </Pressable>
  );
}

/** A Material 3 filter chip. */
function Chip({ label, selected, onPress }: { label: string; selected: boolean; onPress: () => void }) {
  const { c } = useTheme();
  return (
    <View style={{ borderRadius: 8, overflow: 'hidden' }}>
      <Pressable
        onPress={onPress}
        android_ripple={{ color: withAlpha(c.onSurface, 0.12) }}
        accessibilityRole="button"
        accessibilityState={{ selected }}
        style={{
          height: 32,
          paddingHorizontal: selected ? 8 : 16,
          flexDirection: 'row',
          alignItems: 'center',
          gap: 8,
          borderRadius: 8,
          backgroundColor: selected ? c.secondaryContainer : 'transparent',
          borderWidth: selected ? 0 : 1,
          borderColor: c.outlineVariant,
        }}
      >
        {selected ? <Icon name="check" size={18} color={c.onSecondaryContainer} /> : null}
        <Txt role="labelLarge" color={selected ? c.onSecondaryContainer : c.onSurfaceVariant}>
          {label}
        </Txt>
      </Pressable>
    </View>
  );
}
