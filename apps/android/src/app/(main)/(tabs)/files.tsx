import { FlashList } from '@shopify/flash-list';
import { useFocusEffect } from 'expo-router';
import { useCallback, useMemo, useState } from 'react';
import { Alert, BackHandler, Modal, Pressable, RefreshControl, ScrollView, View } from 'react-native';
import { type Edit, errorText, type FolderEntry, type LibraryItem } from 'ursa-core';

import { openFile, saveFile, shareFile } from '@/core/files';
import { editTree, loadLibrary, remove, subtree, useLibrary } from '@/core/library';
import { pickFiles } from '@/core/pick';
import { bytes, dayLabel, fileIcon, isMedia } from '@/format';
import { useTheme } from '@/theme';
import { Button, Empty, Fab, Field, Icon, IconButton, ListRow, Notice, Progress, Sheet, TopBar, Txt, withAlpha } from '@/ui/kit';
import { BackupButton, UploadStrip } from '@/ui/upload-status';

type Sort = 'added' | 'name' | 'size';

/** What the list shows: the folder's subfolders, then its files. */
type Row = { kind: 'folder'; folder: FolderEntry } | { kind: 'file'; item: LibraryItem };

/** Something a menu acts on. */
type Target = { kind: 'folder'; folder: FolderEntry } | { kind: 'file'; item: LibraryItem };

const nameOf = (t: Target) => (t.kind === 'folder' ? t.folder.name : t.item.name);

export default function Files() {
  const items = useLibrary((s) => s.items);
  const folders = useLibrary((s) => s.folders);
  const libLoading = useLibrary((s) => s.loading && !s.loaded);
  const pulling = useLibrary((s) => s.pulling);
  const libError = useLibrary((s) => s.error);
  const refresh = () => loadLibrary({ fromNodes: true, reason: 'pull' });
  const { c } = useTheme();
  const [all, setAll] = useState(false);
  const [sort, setSort] = useState<Sort>('added');
  /** The folder shown; null is the top. */
  const [dir, setDir] = useState<string | null>(null);
  const [menu, setMenu] = useState<Target | null>(null);
  const [sortMenu, setSortMenu] = useState(false);
  const [naming, setNaming] = useState<{ title: string; initial: string; action: string; run: (name: string) => Promise<void> } | null>(null);
  const [moving, setMoving] = useState<Target | null>(null);
  const [working, setWorking] = useState<{ id: string; progress: number } | null>(null);
  const [error, setError] = useState<string | null>(null);

  // A folder deleted here or on another device: back to the top.
  const here = dir ? folders.find((f) => f.id === dir) ?? null : null;
  const current = here ? here.id : null;

  const up = useCallback(() => {
    if (!here) return false;
    setDir(here.parent);
    return true;
  }, [here]);

  // The system back gesture goes up a folder before it leaves the tab.
  useFocusEffect(
    useCallback(() => {
      const sub = BackHandler.addEventListener('hardwareBackPress', up);
      return () => sub.remove();
    }, [up]),
  );

  const rows = useMemo<Row[]>(() => {
    const subfolders = folders
      .filter((f) => f.parent === current)
      .sort((a, b) => a.name.localeCompare(b.name))
      .map((folder) => ({ kind: 'folder' as const, folder }));
    // At the top, "Documents" leaves out photos and videos (the Photos tab has them); inside a
    // folder everything in it shows.
    const list = items.filter((i) => i.dir === current && (all || current !== null || !isMedia(i.mime)));
    if (sort === 'name') list.sort((a, b) => a.name.localeCompare(b.name));
    else if (sort === 'size') list.sort((a, b) => b.size - a.size);
    else list.sort((a, b) => b.addedMs - a.addedMs);
    return [...subfolders, ...list.map((item) => ({ kind: 'file' as const, item }))];
  }, [items, folders, current, all, sort]);

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

  async function edit(edits: Edit[]) {
    setError(null);
    try {
      await editTree(edits);
    } catch (e) {
      setError(errorText(e));
    }
  }

  function newFolder() {
    setNaming({ title: 'New folder', initial: 'New folder', action: 'Create', run: (name) => edit([{ op: 'create_folder', parent: current, name }]) });
  }

  function rename(t: Target) {
    setNaming({
      title: 'Rename',
      initial: nameOf(t),
      action: 'Rename',
      run: (name) =>
        edit([t.kind === 'folder' ? { op: 'rename_folder', id: t.folder.id, name } : { op: 'rename_file', file_id: t.item.fileId, name }]),
    });
  }

  function confirmDelete(t: Target) {
    if (t.kind === 'file') {
      Alert.alert('Delete this file?', `${t.item.name} is removed from every device and node.`, [
        { text: 'Cancel', style: 'cancel' },
        { text: 'Delete', style: 'destructive', onPress: () => remove(t.item).catch((e) => setError(errorText(e))) },
      ]);
      return;
    }
    const inside = subtree(folders, t.folder.id);
    const n = items.filter((i) => i.dir !== null && inside.has(i.dir)).length;
    Alert.alert(
      'Delete this folder?',
      n > 0 ? `${t.folder.name} and the ${n} file${n === 1 ? '' : 's'} in it are removed from every device and node.` : `${t.folder.name} is removed from every device.`,
      [
        { text: 'Cancel', style: 'cancel' },
        { text: 'Delete', style: 'destructive', onPress: () => edit([{ op: 'delete_folder', id: t.folder.id }]) },
      ],
    );
  }

  const crumbs = useMemo(() => {
    const chain: FolderEntry[] = [];
    for (let at = here; at; at = folders.find((f) => f.id === at?.parent) ?? null) chain.unshift(at);
    return chain;
  }, [here, folders]);

  return (
    <View style={{ flex: 1, backgroundColor: c.surface }}>
      <TopBar
        title={here ? here.name : 'Files'}
        leading={here ? <IconButton icon="arrow-back" label="Up a folder" onPress={up} /> : undefined}
        actions={
          <>
            <IconButton icon="create-new-folder" label="New folder" onPress={newFolder} />
            <IconButton icon="sort" label="Sort" onPress={() => setSortMenu(true)} />
            <BackupButton />
          </>
        }
      />
      {here ? (
        <ScrollView horizontal showsHorizontalScrollIndicator={false} contentContainerStyle={{ paddingHorizontal: 16, paddingBottom: 8, gap: 4, alignItems: 'center' }}>
          <Pressable onPress={() => setDir(null)} accessibilityRole="button">
            <Txt role="labelLarge" color={c.primary}>
              Files
            </Txt>
          </Pressable>
          {crumbs.map((f) => (
            <View key={f.id} style={{ flexDirection: 'row', alignItems: 'center', gap: 4 }}>
              <Icon name="chevron-right" size={18} />
              <Pressable onPress={() => setDir(f.id)} accessibilityRole="button">
                <Txt role="labelLarge" color={f.id === here.id ? c.onSurface : c.primary}>
                  {f.name}
                </Txt>
              </Pressable>
            </View>
          ))}
        </ScrollView>
      ) : (
        <View style={{ flexDirection: 'row', gap: 8, paddingHorizontal: 16, paddingBottom: 8 }}>
          <Chip label="Documents" selected={!all} onPress={() => setAll(false)} />
          <Chip label="Everything" selected={all} onPress={() => setAll(true)} />
        </View>
      )}
      <UploadStrip />
      {libError || error ? (
        <View style={{ paddingHorizontal: 16 }}>
          <Notice tone="error" text={error ?? `Could not reach your nodes; this is what this phone knows. ${libError}`} />
        </View>
      ) : null}
      <FlashList
        data={rows}
        keyExtractor={(r) => (r.kind === 'folder' ? `d:${r.folder.id}` : `f:${r.item.fileId}`)}
        getItemType={(r) => r.kind}
        refreshControl={<RefreshControl refreshing={pulling} onRefresh={refresh} colors={[c.primary]} progressBackgroundColor={c.surfaceContainerHigh} />}
        contentContainerStyle={{ paddingBottom: 96 }}
        renderItem={({ item: row }) =>
          row.kind === 'folder' ? (
            <FolderRow folder={row.folder} onPress={() => setDir(row.folder.id)} onMore={() => setMenu(row)} />
          ) : (
            <FileRow
              item={row.item}
              progress={working?.id === row.item.fileId ? working.progress : null}
              onPress={() => act(row.item, openFile)}
              onMore={() => setMenu(row)}
            />
          )
        }
        ListEmptyComponent={
          libLoading ? null : (
            <Empty
              icon="folder-open"
              title={here ? 'This folder is empty' : 'No files yet'}
              body="Documents, archives, anything: encrypted on this phone and kept across your pool."
              action={<Button label="Upload files" icon="upload-file" onPress={() => pickFiles(current)} style={{ marginTop: 8 }} />}
            />
          )
        }
      />
      <Fab icon="upload-file" label="Upload" onPress={() => pickFiles(current)} />

      <Sheet
        visible={menu !== null}
        title={menu ? nameOf(menu) : undefined}
        onClose={() => setMenu(null)}
        actions={
          !menu
            ? []
            : menu.kind === 'folder'
              ? [
                  { icon: 'folder-open', label: 'Open', onPress: () => setDir(menu.folder.id) },
                  { icon: 'drive-file-rename-outline', label: 'Rename', onPress: () => rename(menu) },
                  { icon: 'drive-file-move', label: 'Move to…', onPress: () => setMoving(menu) },
                  { icon: 'delete-outline', label: 'Delete', danger: true, onPress: () => confirmDelete(menu) },
                ]
              : [
                  { icon: 'open-in-new', label: 'Open', onPress: () => act(menu.item, openFile) },
                  { icon: 'share', label: 'Share', onPress: () => act(menu.item, shareFile) },
                  { icon: 'download', label: 'Save to this phone', onPress: () => act(menu.item, saveFile) },
                  { icon: 'drive-file-rename-outline', label: 'Rename', onPress: () => rename(menu) },
                  { icon: 'drive-file-move', label: 'Move to…', onPress: () => setMoving(menu) },
                  { icon: 'delete-outline', label: 'Delete', danger: true, onPress: () => confirmDelete(menu) },
                ]
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
      <MoveSheet
        target={moving}
        folders={folders}
        onClose={() => setMoving(null)}
        onPick={(to) => {
          const t = moving;
          setMoving(null);
          if (!t) return;
          edit([t.kind === 'folder' ? { op: 'move_folder', id: t.folder.id, parent: to } : { op: 'move_file', file_id: t.item.fileId, dir: to }]);
        }}
      />
      <NameDialog
        key={naming ? `${naming.title}:${naming.initial}` : 'closed'}
        request={naming}
        onClose={() => setNaming(null)}
      />
    </View>
  );
}

/** Where to move a file or folder: every folder but the moved folder's own subtree. */
function MoveSheet({ target, folders, onClose, onPick }: { target: Target | null; folders: FolderEntry[]; onClose: () => void; onPick: (to: string | null) => void }) {
  const { c } = useTheme();
  const choices = useMemo(() => {
    if (!target) return [];
    const blocked = target.kind === 'folder' ? subtree(folders, target.folder.id) : new Set<string>();
    return folders.filter((f) => !blocked.has(f.id)).sort((a, b) => a.path.localeCompare(b.path));
  }, [target, folders]);
  return (
    <Modal visible={target !== null} transparent animationType="slide" onRequestClose={onClose} statusBarTranslucent navigationBarTranslucent>
      <Pressable style={{ flex: 1, backgroundColor: withAlpha(c.scrim, 0.32) }} onPress={onClose} accessibilityLabel="Close" />
      <View style={{ backgroundColor: c.surfaceContainerLow, borderTopLeftRadius: 28, borderTopRightRadius: 28, maxHeight: '70%', paddingBottom: 16 }}>
        <Txt role="titleMedium" style={{ paddingHorizontal: 24, paddingTop: 24, paddingBottom: 8 }} numberOfLines={1}>
          {target ? `Move ${nameOf(target)} to` : ''}
        </Txt>
        <ScrollView>
          <ListRow icon="home" title="Files" subtitle="The top" onPress={() => onPick(null)} />
          {choices.map((f) => (
            <ListRow key={f.id} icon="folder" title={f.name} subtitle={f.path} onPress={() => onPick(f.id)} />
          ))}
        </ScrollView>
      </View>
    </Modal>
  );
}

/** Asks for a name (a new folder's, or a new one for something). */
function NameDialog({ request, onClose }: { request: { title: string; initial: string; action: string; run: (name: string) => Promise<void> } | null; onClose: () => void }) {
  const { c } = useTheme();
  const [name, setName] = useState(request?.initial ?? '');
  const ok = name.trim().length > 0;
  const submit = () => {
    if (!request || !ok) return;
    onClose();
    request.run(name.trim());
  };
  return (
    <Modal visible={request !== null} transparent animationType="fade" onRequestClose={onClose} statusBarTranslucent>
      <View style={{ flex: 1, backgroundColor: withAlpha(c.scrim, 0.32), justifyContent: 'center', padding: 24 }}>
        <View style={{ backgroundColor: c.surfaceContainerHigh, borderRadius: 28, padding: 24, gap: 8 }}>
          <Txt role="headlineSmall">{request?.title ?? ''}</Txt>
          <Field label="Name" value={name} onChangeText={setName} autoFocus selectTextOnFocus onSubmitEditing={submit} returnKeyType="done" />
          <View style={{ flexDirection: 'row', justifyContent: 'flex-end', gap: 8 }}>
            <Button label="Cancel" kind="text" onPress={onClose} />
            <Button label={request?.action ?? 'OK'} kind="text" onPress={submit} disabled={!ok} />
          </View>
        </View>
      </View>
    </Modal>
  );
}

function FolderRow({ folder, onPress, onMore }: { folder: FolderEntry; onPress: () => void; onMore: () => void }) {
  const { c } = useTheme();
  return (
    <Pressable
      onPress={onPress}
      onLongPress={onMore}
      android_ripple={{ color: withAlpha(c.onSurface, 0.08) }}
      style={{ flexDirection: 'row', alignItems: 'center', paddingLeft: 16, paddingRight: 4, paddingVertical: 10, gap: 16 }}
    >
      <View style={{ width: 40, height: 40, borderRadius: 12, backgroundColor: c.primaryContainer, alignItems: 'center', justifyContent: 'center' }}>
        <Icon name="folder" color={c.onPrimaryContainer} size={22} />
      </View>
      <Txt role="bodyLarge" numberOfLines={1} style={{ flex: 1 }}>
        {folder.name}
      </Txt>
      <IconButton icon="more-vert" label={`More for ${folder.name}`} onPress={onMore} />
    </Pressable>
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
