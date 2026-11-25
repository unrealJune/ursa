import { router } from 'expo-router';
import { useState } from 'react';
import { errorText, Ursa } from 'ursa-core';

import { refreshStatus } from '@/core/session';
import { useTheme } from '@/theme';
import { CodeField } from '@/ui/code-field';
import { Button, Field, Notice, Page, Txt } from '@/ui/kit';

export default function AddNode() {
  const { c } = useTheme();
  const [link, setLink] = useState('');
  const [label, setLabel] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  return (
    <Page>
      <Txt color={c.onSurfaceVariant} style={{ marginVertical: 8 }}>
        A node prints its claim link when it starts. Claiming it makes your account its owner; after that, run repair so files use it.
      </Txt>
      <CodeField label="Claim link" value={link} onChange={setLink} supporting="Starts with ursa-claim:" />
      <Field label="Name" value={label} onChangeText={setLabel} placeholder="Home server" />
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
            await Ursa.claimNode(link.trim(), label.trim());
            await refreshStatus();
            router.back();
          } catch (e) {
            setError(errorText(e));
            setBusy(false);
          }
        }}
      />
    </Page>
  );
}
