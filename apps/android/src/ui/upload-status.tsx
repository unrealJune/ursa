import { router } from 'expo-router';
import { Pressable, View } from 'react-native';

import { useUrsa } from '@/core/store';
import { useTheme } from '@/theme';
import { Icon, IconButton, Progress, Txt, withAlpha } from '@/ui/kit';

/** The top-bar backup button: what Google Photos shows by the avatar. */
export function BackupButton() {
  const { uploads, backupScanning, prefs } = useUrsa();
  const { c } = useTheme();
  const busy = uploads.current !== null || backupScanning;
  const icon = busy ? 'cloud-upload' : uploads.failed > 0 ? 'cloud-off' : prefs.backup ? 'cloud-done' : 'cloud-queue';
  return <IconButton icon={icon} label="Backup" color={busy ? c.primary : undefined} onPress={() => router.push('/backup')} />;
}

/** A strip under the top bar while files are going up. */
export function UploadStrip() {
  const { uploads, backupScanning } = useUrsa();
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
