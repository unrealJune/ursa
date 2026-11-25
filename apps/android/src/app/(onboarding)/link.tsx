import { useEffect, useState } from 'react';
import { View } from 'react-native';
import { errorText, Ursa } from 'ursa-core';

import { adopt } from '@/core/session';
import { useTheme } from '@/theme';
import { CodeField } from '@/ui/code-field';
import { Button, Loading, Notice, Page, Txt } from '@/ui/kit';

/** Joins an account another device offers: scan or paste its link code, then match the confirmation code. */
export default function Link() {
  const { c } = useTheme();
  const [code, setCode] = useState('');
  const [confirm, setConfirm] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const sub = Ursa.addListener('onLinkConfirm', (e) => setConfirm(e.code));
    return () => sub.remove();
  }, []);

  async function join() {
    setBusy(true);
    setError(null);
    setConfirm(null);
    try {
      await adopt(await Ursa.linkJoin(code.trim()));
    } catch (e) {
      setError(errorText(e));
      setBusy(false);
      setConfirm(null);
    }
  }

  if (busy) {
    return (
      <Page>
        <View style={{ alignItems: 'center', gap: 20, marginTop: 48 }}>
          {confirm ? (
            <>
              <Txt role="titleMedium" style={{ textAlign: 'center' }}>
                Check that your other device shows
              </Txt>
              <Txt role="displaySmall" style={{ letterSpacing: 6, fontFamily: 'monospace', color: c.primary }}>
                {confirm}
              </Txt>
              <Txt color={c.onSurfaceVariant} style={{ textAlign: 'center' }}>
                If it matches, approve this phone there. If it does not, deny it: someone else has the link code.
              </Txt>
              <Loading />
            </>
          ) : (
            <>
              <Loading />
              <Txt color={c.onSurfaceVariant}>Reaching your other device…</Txt>
            </>
          )}
        </View>
      </Page>
    );
  }

  return (
    <Page>
      <Txt role="headlineMedium" style={{ marginTop: 8 }}>
        Link to your account
      </Txt>
      <Txt color={c.onSurfaceVariant} style={{ marginVertical: 12 }}>
        On a device that is already signed in, open Settings, then Devices, then Add a device. Scan the code it shows, or paste it here.
      </Txt>
      <CodeField label="Link code" value={code} onChange={setCode} supporting="Starts with ursa-link:" />
      {error ? <Notice tone="error" text={error} /> : null}
      <Button label="Link" style={{ marginTop: 16 }} disabled={!code.trim().startsWith('ursa-link:')} onPress={join} />
    </Page>
  );
}
