import { router } from 'expo-router';
import { Pressable, View } from 'react-native';

import { clearBulkError, useLibrary } from '@/core/library';
import { useSession } from '@/core/session';
import { useUploads } from '@/core/uploads';
import { useTheme } from '@/theme';
import { Button, Icon, IconButton, Notice, Progress, Txt, withAlpha } from '@/ui/kit';

/** The top-bar backup button: what Google Photos shows by the avatar. */
export function BackupButton() {
  const busy = useUploads((u) => u.current !== null || u.scanning);
  const failed = useUploads((u) => u.failed > 0);
  const backup = useSession((s) => s.prefs.backup);
  const { c } = useTheme();
  const icon = busy ? 'cloud-upload' : failed ? 'cloud-off' : backup ? 'cloud-done' : 'cloud-queue';
  return <IconButton icon={icon} label="Backup" color={busy ? c.primary : undefined} onPress={() => router.push('/backup')} />;
}

/** A strip under the top bar while files are going up. */
export function UploadStrip() {
  const uploads = useUploads((u) => u);
  const backupScanning = uploads.scanning;
  const { c } = useTheme();
  const cur = uploads.current;
  if (!cur && !backupScanning) return null;
  const n = Math.min(uploads.done + uploads.failed + 1, uploads.total);
  const label = cur ? `Backing up ${n} of ${uploads.total}` : 'Looking for new photos…';
  return (
    <Pressable
      onPress={() => router.push('/backup')}
      android_ripple={{ color: withAlpha(c.onSurface, 0.08) }}
      style={{ marginHorizontal: 16, marginBottom: 8, padding: 12, borderRadius: 16, backgroundColor: c.surfaceContainer, gap: 8 }}
    >
      <View style={{ flexDirection: 'row', alignItems: 'center', gap: 12 }}>
        <Icon name="cloud-upload" size={20} color={c.primary} />
        <View style={{ flex: 1 }}>
          <Txt role="labelLarge">{label}</Txt>
          {cur ? (
            <Txt role="bodySmall" color={c.onSurfaceVariant} numberOfLines={1}>
              {cur.name}
            </Txt>
          ) : null}
        </View>
      </View>
      <Progress progress={cur && cur.total > 0 ? cur.done / cur.total : null} />
    </Pressable>
  );
}

/** A strip while a batch (delete, save) runs, and what it could not do afterwards. */
export function BulkStrip() {
  const bulk = useLibrary((s) => s.bulk);
  const bulkError = useLibrary((s) => s.bulkError);
  const { c } = useTheme();
  if (bulkError && !bulk) {
    return (
      <View style={{ paddingHorizontal: 16 }}>
        <Notice tone="error" text={bulkError} action={<Button label="Dismiss" kind="text" onPress={clearBulkError} style={{ alignSelf: 'flex-start' }} />} />
      </View>
    );
  }
  if (!bulk) return null;
  const n = bulk.done + bulk.failed;
  return (
    <View style={{ marginHorizontal: 16, marginBottom: 8, padding: 12, borderRadius: 16, backgroundColor: c.surfaceContainer, gap: 8 }}>
      <Txt role="labelLarge">{bulk.total > 1 ? `${bulk.label}: ${n} of ${bulk.total}` : `${bulk.label}…`}</Txt>
      <Progress progress={bulk.total > 1 && n > 0 ? n / bulk.total : null} />
    </View>
  );
}
