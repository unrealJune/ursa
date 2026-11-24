import * as Clipboard from 'expo-clipboard';
import { Fragment, useEffect, useState } from 'react';
import { Share, View } from 'react-native';
import QRCode from 'react-native-qrcode-svg';
import { errorText, Ursa } from 'ursa-core';

import { useUrsa } from '@/core/store';
import { shortId } from '@/format';
import { useTheme } from '@/theme';
import { CodeField } from '@/ui/code-field';
import { Button, Divider, Field, Group, ListRow, Notice, Page, SectionHeader, Txt } from '@/ui/kit';

/**
 * Friends share space both ways: adding someone's invite grants them space on your nodes and
 * puts their nodes in your pool (they add yours to theirs the same way).
 */
export default function Friends() {
  const { status, refreshStatus } = useUrsa();
  const { c } = useTheme();
  const [invite, setInvite] = useState<string | null>(null);
  const [theirs, setTheirs] = useState('');
  const [quota, setQuota] = useState('50');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [added, setAdded] = useState<string | null>(null);

  useEffect(() => {
    Ursa.invite().then(setInvite, (e) => setError(errorText(e)));
  }, []);

  const friends = status?.friends ?? [];
  const gb = Number(quota);

  return (
    <Page padded={false}>
      <SectionHeader>Your invite</SectionHeader>
      <View style={{ marginHorizontal: 16, padding: 20, borderRadius: 24, backgroundColor: c.surfaceContainer, alignItems: 'center', gap: 16 }}>
        <Txt color={c.onSurfaceVariant} style={{ textAlign: 'center' }}>
          A friend scans this to add you. It holds your name and your nodes’ addresses, nothing secret.
        </Txt>
        {invite ? (
          <View style={{ padding: 16, borderRadius: 20, backgroundColor: '#FFFFFF' }}>
            <QRCode value={invite} size={200} ecl="M" />
          </View>
        ) : null}
        <View style={{ flexDirection: 'row', gap: 12 }}>
          <Button label="Copy" kind="tonal" icon="content-copy" disabled={!invite} onPress={() => invite && Clipboard.setStringAsync(invite)} />
          <Button label="Share" kind="tonal" icon="share" disabled={!invite} onPress={() => invite && Share.share({ message: invite })} />
        </View>
      </View>

      <SectionHeader>Add a friend</SectionHeader>
      <View style={{ marginHorizontal: 16 }}>
        <CodeField label="Their invite" value={theirs} onChange={setTheirs} supporting="Starts with ursa-invite:" />
        <Field
          label="Space you give them, in GB"
          value={quota}
          onChangeText={setQuota}
          keyboardType="number-pad"
          error={quota && !(gb > 0) ? 'Give at least 1 GB.' : null}
          supporting="On each of your nodes. You can share more later."
        />
        {error ? <Notice tone="error" text={error} /> : null}
        {added ? <Notice text={`${added} is now your friend. Run repair so your files use their nodes too.`} /> : null}
        <Button
          label="Add friend"
          icon="person-add"
          style={{ marginTop: 8, alignSelf: 'flex-start' }}
          disabled={!theirs.trim().startsWith('ursa-invite:') || !(gb > 0)}
          busy={busy}
          onPress={async () => {
            setBusy(true);
            setError(null);
            setAdded(null);
            try {
              const f = await Ursa.befriend(theirs.trim(), Math.round(gb));
              setAdded(f.name || shortId(f.accountId));
              setTheirs('');
              await refreshStatus();
            } catch (e) {
              setError(errorText(e));
            } finally {
              setBusy(false);
            }
          }}
        />
      </View>

      {friends.length ? (
        <>
          <SectionHeader>Friends</SectionHeader>
          <Group>
            {friends.map((f, i) => (
              <Fragment key={f.accountId}>
                {i > 0 ? <Divider inset={56} /> : null}
                <ListRow icon="person" title={f.name || shortId(f.accountId)} subtitle={`${f.nodes} ${f.nodes === 1 ? 'node' : 'nodes'}`} />
              </Fragment>
            ))}
          </Group>
        </>
      ) : null}
    </Page>
  );
}
