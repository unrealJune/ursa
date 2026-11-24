import { router } from 'expo-router';
import { useState } from 'react';
import { errorText, Ursa } from 'ursa-core';

import { useUrsa } from '@/core/store';
import { useTheme } from '@/theme';
import { Button, Field, Notice, Page, Txt } from '@/ui/kit';

export default function Create() {
  const { adopt } = useUrsa();
  const { c } = useTheme();
  const [name, setName] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  return (
    <Page>
      <Txt role="headlineMedium" style={{ marginTop: 8 }}>
        Create your account
      </Txt>
      <Txt color={c.onSurfaceVariant} style={{ marginVertical: 12 }}>
        Your name is what friends see when you share space with them. Only your devices can read it.
      </Txt>
      <Field label="Your name" value={name} onChangeText={setName} autoFocus autoCapitalize="words" returnKeyType="done" />
      {error ? <Notice tone="error" text={error} /> : null}
      <Button
        label="Create"
        style={{ marginTop: 16 }}
        disabled={!name.trim()}
        busy={busy}
        onPress={async () => {
          setBusy(true);
          setError(null);
          try {
            await adopt(await Ursa.createAccount(name.trim()));
            router.replace('/words');
          } catch (e) {
            setError(errorText(e));
            setBusy(false);
          }
        }}
      />
    </Page>
  );
}
