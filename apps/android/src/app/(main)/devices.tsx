import { router } from 'expo-router';

import { useSession } from '@/core/session';
import { dateTime } from '@/format';
import { useTheme } from '@/theme';
import { Button, Divider, Group, ListRow, Page, Txt } from '@/ui/kit';
import { Fragment } from 'react';

function platformIcon(p: string): string {
  if (p === 'android' || p === 'ios') return 'smartphone';
  if (p === 'windows' || p === 'macos' || p === 'linux') return 'computer';
  return 'devices-other';
}

export default function Devices() {
  const status = useSession((s) => s.status);
  const { c } = useTheme();
  const devices = status?.devices ?? [];
  return (
    <Page padded={false}>
      <Txt color={c.onSurfaceVariant} style={{ marginHorizontal: 16, marginVertical: 8 }}>
        Every device here holds your account and can read your files.
      </Txt>
      <Group>
        {devices.map((d, i) => (
          <Fragment key={d.deviceId}>
            {i > 0 ? <Divider inset={56} /> : null}
            <ListRow icon={platformIcon(d.platform)} title={d.thisDevice ? `${d.name} (this phone)` : d.name} subtitle={`${d.platform} · added ${dateTime(d.addedMs)}`} />
          </Fragment>
        ))}
      </Group>
      <Button label="Add a device" icon="add" onPress={() => router.push('/add-device')} style={{ alignSelf: 'flex-start', margin: 16 }} />
    </Page>
  );
}
