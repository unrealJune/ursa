import { Image } from 'expo-image';
import { Redirect, router } from 'expo-router';
import { View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { useUrsa } from '@/core/store';
import { useTheme } from '@/theme';
import { Button, Icon, Txt } from '@/ui/kit';

export default function Welcome() {
  const { phase } = useUrsa();
  const { c } = useTheme();
  const insets = useSafeAreaInsets();

  // An account without a node of its own (a restart mid-onboarding) picks up there.
  if (phase === 'needs-node') return <Redirect href="/node" />;

  const point = (icon: string, title: string, body: string) => (
    <View style={{ flexDirection: 'row', gap: 16, alignItems: 'flex-start' }}>
      <Icon name={icon} color={c.primary} />
      <View style={{ flex: 1 }}>
        <Txt role="titleMedium">{title}</Txt>
        <Txt color={c.onSurfaceVariant}>{body}</Txt>
      </View>
    </View>
  );

  return (
    <View style={{ flex: 1, backgroundColor: c.surface, paddingTop: insets.top + 48, paddingBottom: insets.bottom + 24, paddingHorizontal: 24 }}>
      <View style={{ flex: 1, gap: 28 }}>
        <Image source={require('../../../assets/icon.png')} style={{ width: 72, height: 72, borderRadius: 22 }} />
        <Txt role="displaySmall">Your photos, kept among friends</Txt>
        <View style={{ gap: 20 }}>
          {point('lock-outline', 'Encrypted on this phone', 'Your files are sealed before they leave. Nodes, friends and relays only ever hold ciphertext.')}
          {point('groups', 'Spread across your friends', 'Each file is split into pieces on your nodes and your friends’. Any three of five bring it back.')}
          {point('key', 'Yours to recover', 'Recovery words or a key file restore everything, even if your home node is gone.')}
        </View>
      </View>
      <View style={{ gap: 12 }}>
        <Button label="Create an account" onPress={() => router.push('/create')} />
        <Button label="I already have an account" kind="tonal" onPress={() => router.push('/sign-in')} />
        <Button label="Network" kind="text" icon="lan" style={{ alignSelf: 'center' }} onPress={() => router.push('/network')} />
      </View>
    </View>
  );
}
