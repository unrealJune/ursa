import { router } from 'expo-router';

import { useTheme } from '@/theme';
import { Group, ListRow, Divider, Page, Txt } from '@/ui/kit';

export default function SignIn() {
  const { c } = useTheme();
  return (
    <Page padded={false}>
      <Txt role="headlineMedium" style={{ marginTop: 8, marginHorizontal: 16 }}>
        Sign in to your account
      </Txt>
      <Txt color={c.onSurfaceVariant} style={{ marginVertical: 12, marginHorizontal: 16 }}>
        Ursa has no passwords and no server. Use another of your devices, or what you saved when you made the account.
      </Txt>
      <Group style={{ marginTop: 8 }}>
        <ListRow icon="devices" title="Link from another device" subtitle="Your computer or another phone shows a code" onPress={() => router.push('/link')} />
        <Divider inset={56} />
        <ListRow icon="password" title="Recovery words" subtitle="The 24 words you wrote down" onPress={() => router.push({ pathname: '/recover', params: { with: 'words' } })} />
        <Divider inset={56} />
        <ListRow icon="key" title="Key file" subtitle="A .ursa-key file and its passphrase" onPress={() => router.push({ pathname: '/recover', params: { with: 'file' } })} />
      </Group>
    </Page>
  );
}
