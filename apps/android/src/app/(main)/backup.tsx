import { View } from 'react-native';

import { pickAndUpload, pickFiles } from '@/core/pick';
import { useUrsa } from '@/core/store';
import { useTheme } from '@/theme';
import { Button, Divider, Group, ListRow, Notice, Page, Progress, SectionHeader, Switch, Txt } from '@/ui/kit';

export default function Backup() {
  const { prefs, setPrefs, uploads, backupScanning, backupNow, enqueue } = useUrsa();
  const { c } = useTheme();
  const cur = uploads.current;

  return (
    <Page padded={false}>
      <View style={{ marginHorizontal: 16, marginVertical: 8, padding: 20, borderRadius: 24, backgroundColor: c.primaryContainer, gap: 12 }}>
        <Txt role="titleLarge" color={c.onPrimaryContainer}>
          {cur ? `Backing up ${Math.min(uploads.done + uploads.failed + 1, uploads.total)} of ${uploads.total}` : backupScanning ? 'Looking for new photos…' : 'Backup is up to date'}
        </Txt>
        {cur ? (
          <>
            <Txt color={c.onPrimaryContainer} numberOfLines={1}>
              {cur.name}
            </Txt>
            <Progress progress={cur.total > 0 ? cur.done / cur.total : null} />
          </>
        ) : (
          <Txt color={c.onPrimaryContainer}>
            {uploads.total > 0 ? `${uploads.done} uploaded${uploads.failed ? `, ${uploads.failed} failed` : ''} this session.` : 'Everything you upload is encrypted on this phone first.'}
          </Txt>
        )}
      </View>
      {uploads.lastError ? (
        <View style={{ paddingHorizontal: 16 }}>
          <Notice tone="error" text={uploads.lastError} />
        </View>
      ) : null}

      <SectionHeader>Camera roll</SectionHeader>
      <Group>
        <ListRow
          icon="photo-camera"
          title="Back up this phone’s photos"
          subtitle="New photos go up whenever Ursa is open"
          trailing={<Switch value={prefs.backup} onChange={(v) => setPrefs({ backup: v })} />}
        />
        <Divider inset={56} />
        <ListRow icon="videocam" title="Include videos" subtitle="Videos use far more space in the pool" trailing={<Switch value={prefs.backupVideos} onChange={(v) => setPrefs({ backupVideos: v })} />} />
      </Group>
      {prefs.backup ? <Button label="Back up now" kind="tonal" icon="sync" busy={backupScanning} onPress={backupNow} style={{ alignSelf: 'flex-start', marginHorizontal: 16, marginTop: 16 }} /> : null}

      <SectionHeader>Upload by hand</SectionHeader>
      <Group>
        <ListRow icon="add-photo-alternate" title="Photos and videos" onPress={() => pickAndUpload(enqueue)} />
        <Divider inset={56} />
        <ListRow icon="upload-file" title="Files" onPress={() => pickFiles(enqueue)} />
      </Group>
    </Page>
  );
}
