import { useState } from 'react';
import { View } from 'react-native';
import { errorText, Ursa } from 'ursa-core';

import { refreshStatus } from '@/core/session';
import { useTheme } from '@/theme';
import { CodeField } from '@/ui/code-field';
import { Button, Field, Icon, Notice, Page, Txt } from '@/ui/kit';

/**
 * The last onboarding step: an account needs at least one node of its own, so it gives space to
 * the pool as well as taking it.
 */
export default function AddNode() {
  const { c } = useTheme();
  const [link, setLink] = useState('');
  const [label, setLabel] = useState('Home node');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  return (
    <Page>
      <View style={{ width: 56, height: 56, borderRadius: 16, backgroundColor: c.primaryContainer, alignItems: 'center', justifyContent: 'center', marginTop: 8 }}>
        <Icon name="dns" color={c.onPrimaryContainer} size={28} />
      </View>
      <Txt role="headlineMedium" style={{ marginTop: 16 }}>
        Add a node you own
      </Txt>
      <Txt color={c.onSurfaceVariant} style={{ marginVertical: 12 }}>
        A node is an always-on computer that stores encrypted pieces of your files and your friends’. Everyone in the pool brings one, so the
        pool has room for everyone.
      </Txt>
      <Txt color={c.onSurfaceVariant} style={{ marginBottom: 12 }}>
        Run ursa-node on a home server, a NAS or a spare PC. It prints a claim link when it starts: scan it or paste it here.
      </Txt>
      <CodeField label="Claim link" value={link} onChange={setLink} supporting="Starts with ursa-claim:" />
      <Field label="Name" value={label} onChangeText={setLabel} />
      {error ? <Notice tone="error" text={error} /> : null}
      <Button
        label="Add node"
        style={{ marginTop: 16 }}
        disabled={!link.trim().startsWith('ursa-claim:')}
        busy={busy}
        onPress={async () => {
          setBusy(true);
          setError(null);
          try {
            await Ursa.claimNode(link.trim(), label.trim() || 'Home node');
            await refreshStatus();
          } catch (e) {
            setError(errorText(e));
            setBusy(false);
          }
        }}
      />
    </Page>
  );
}
