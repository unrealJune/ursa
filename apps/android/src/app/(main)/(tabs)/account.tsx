import Constants from 'expo-constants';
import { router } from 'expo-router';
import * as Clipboard from 'expo-clipboard';
import { useEffect, useState } from 'react';
import { Alert, View } from 'react-native';

import { useLibrary } from '@/core/library';
import { setPrefs, signOut, useSession } from '@/core/session';
import { telemetryEnabled, telemetryId } from '@/core/telemetry';
import { shortId } from '@/format';
import { useTheme } from '@/theme';
import { Divider, Group, Icon, ListRow, Page, SectionHeader, Switch, TopBar, Txt } from '@/ui/kit';

export default function Account() {
  const status = useSession((s) => s.status);
  const prefs = useSession((s) => s.prefs);
  const count = useLibrary((s) => s.items.length);
  const { c } = useTheme();
  const [telemetry, setTelemetry] = useState<string | null>(null);
  useEffect(() => {
    if (telemetryEnabled) telemetryId().then(setTelemetry);
  }, []);
  if (!status) return null;
  const name = status.name || 'Your account';
  const friends = status.friends.length;

  return (
    <View style={{ flex: 1, backgroundColor: c.surface }}>
      <TopBar title="Account" />
      <Page padded={false}>
        <View style={{ alignItems: 'center', paddingVertical: 16, gap: 8 }}>
          <View style={{ width: 80, height: 80, borderRadius: 40, backgroundColor: c.primaryContainer, alignItems: 'center', justifyContent: 'center' }}>
            <Txt role="headlineLarge" color={c.onPrimaryContainer}>
              {name.trim().charAt(0).toUpperCase()}
            </Txt>
          </View>
          <Txt role="headlineSmall">{name}</Txt>
          <Txt role="bodySmall" color={c.onSurfaceVariant} selectable>
            {status.accountId ? `Account ${shortId(status.accountId)} · ${count} files` : ''}
          </Txt>
        </View>

        <SectionHeader>Backup</SectionHeader>
        <Group>
          <ListRow
            icon="cloud-upload"
            title="Back up this phone’s photos"
            subtitle={prefs.backup ? 'On, while Ursa is open' : 'Off'}
            onPress={() => router.push('/backup')}
            trailing={<Switch value={prefs.backup} onChange={(v) => setPrefs({ backup: v })} />}
          />
        </Group>

        <SectionHeader>Storage</SectionHeader>
        <Group>
          <ListRow icon="dns" title="Nodes" subtitle={`${status.homeNodes.length} of your own · repair`} onPress={() => router.push('/nodes')} />
          <Divider inset={56} />
          <ListRow icon="group" title="Friends" subtitle={friends === 0 ? 'Share space with friends' : `${friends} ${friends === 1 ? 'friend' : 'friends'}`} onPress={() => router.push('/friends')} />
        </Group>

        <SectionHeader>Security</SectionHeader>
        <Group>
          <ListRow icon="devices" title="Devices" subtitle={`${status.devices.length} signed in · add a device`} onPress={() => router.push('/devices')} />
          <Divider inset={56} />
          <ListRow icon="key" title="Recovery words and key file" onPress={() => router.push('/recovery')} />
          <Divider inset={56} />
          <ListRow icon="lan" title="Network" subtitle="Relays and relay token" onPress={() => router.push('/network')} />
        </Group>

        <SectionHeader>This phone</SectionHeader>
        <Group>
          <ListRow icon="smartphone" title="Device id" subtitle={shortId(status.deviceId)} />
          <Divider inset={56} />
          <ListRow icon="info-outline" title="Version" subtitle={Constants.expoConfig?.version ?? ''} />
          {telemetryEnabled && telemetry ? (
            <>
              <Divider inset={56} />
              <ListRow
                icon="monitor-heart"
                title="Telemetry ID"
                subtitle={`${telemetry} · this build sends diagnostics to its developer`}
                onPress={() => Clipboard.setStringAsync(telemetry)}
              />
            </>
          ) : null}
          <Divider inset={56} />
          <ListRow
            icon="logout"
            title="Sign out of this phone"
            danger
            onPress={() =>
              Alert.alert(
                'Sign out?',
                'This phone forgets your account. Your files stay in your pool; to sign in again you need your recovery words, your key file or another of your devices.',
                [
                  { text: 'Cancel', style: 'cancel' },
                  { text: 'Sign out', style: 'destructive', onPress: () => signOut() },
                ],
              )
            }
          />
        </Group>
        <View style={{ flexDirection: 'row', justifyContent: 'center', alignItems: 'center', gap: 8, marginTop: 24 }}>
          <Icon name="lock-outline" size={16} />
          <Txt role="bodySmall" color={c.onSurfaceVariant}>
            End-to-end encrypted, post-quantum
          </Txt>
        </View>
      </Page>
    </View>
  );
}
