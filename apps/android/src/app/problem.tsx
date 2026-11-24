import { router } from 'expo-router';
import { useState } from 'react';
import { Alert, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { useUrsa } from '@/core/store';
import { useTheme } from '@/theme';
import { Button, Icon, Txt } from '@/ui/kit';

/** The core could not start, or this phone's account could not be reached. */
export default function Problem() {
  const { phase, problem, retry, signOut } = useUrsa();
  const { c } = useTheme();
  const insets = useSafeAreaInsets();
  const [busy, setBusy] = useState(false);
  const offline = phase === 'offline';

  return (
    <View style={{ flex: 1, backgroundColor: c.surface, paddingTop: insets.top + 64, paddingBottom: insets.bottom + 24, paddingHorizontal: 24 }}>
      <View style={{ flex: 1, gap: 16 }}>
        <Icon name={offline ? 'cloud-off' : 'error-outline'} size={40} color={c.primary} />
        <Txt role="headlineMedium">{offline ? 'Your nodes are out of reach' : 'Ursa could not start'}</Txt>
        <Txt color={c.onSurfaceVariant}>
          {offline
            ? 'This phone still holds your account, but none of your nodes answered. Check your connection, or the relays and token under Network.'
            : 'The network could not start. A relay address may be wrong.'}
        </Txt>
        {problem ? (
          <Txt role="bodySmall" color={c.onSurfaceVariant} selectable>
            {problem}
          </Txt>
        ) : null}
      </View>
      <View style={{ gap: 12 }}>
        <Button
          label="Try again"
          icon="refresh"
          busy={busy}
          onPress={async () => {
            setBusy(true);
            await retry();
            setBusy(false);
          }}
        />
        <Button label="Network" kind="tonal" icon="lan" onPress={() => router.push('/network')} />
        {offline ? (
          <Button
            label="Sign out of this phone"
            kind="text"
            onPress={() =>
              Alert.alert(
                'Sign out?',
                'This phone forgets your account. Your files stay on your nodes; to sign in again you need your recovery words, your key file or another of your devices.',
                [
                  { text: 'Cancel', style: 'cancel' },
                  { text: 'Sign out', style: 'destructive', onPress: () => signOut() },
                ],
              )
            }
          />
        ) : null}
      </View>
    </View>
  );
}
