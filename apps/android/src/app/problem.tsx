import { router } from 'expo-router';
import { useEffect, useState } from 'react';
import { Alert, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { signOut, start, useSession } from '@/core/session';
import { useTheme } from '@/theme';
import { Button, Icon, Txt } from '@/ui/kit';

/** The core could not start, or this phone's account could not be reached (it keeps trying). */
export default function Problem() {
  const phase = useSession((s) => s.phase);
  const problem = useSession((s) => s.problem);
  const retryAt = useSession((s) => s.retryAt);
  const { c } = useTheme();
  const insets = useSafeAreaInsets();
  const [busy, setBusy] = useState(false);
  const offline = phase === 'offline';
  const wait = useCountdown(retryAt);

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
        {offline ? (
          <Txt role="labelLarge" color={c.primary}>
            {busy || retryAt === null ? 'Trying again…' : `Trying again in ${wait} s`}
          </Txt>
        ) : null}
        {problem ? (
          <Txt role="bodySmall" color={c.onSurfaceVariant} selectable>
            {problem}
          </Txt>
        ) : null}
      </View>
      <View style={{ gap: 12 }}>
        <Button
          label="Try again now"
          icon="refresh"
          busy={busy}
          onPress={async () => {
            setBusy(true);
            await start();
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

/** Whole seconds until `at`, ticking. */
function useCountdown(at: number | null): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (at === null) return;
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [at]);
  return at === null ? 0 : Math.max(0, Math.ceil((at - now) / 1000));
}
