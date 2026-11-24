import { useEffect, useState } from 'react';
import { errorText } from 'ursa-core';

import { DEFAULT_RELAYS, relayToken } from '@/core/settings';
import { setNetwork, useSession } from '@/core/session';
import { useTheme } from '@/theme';
import { Button, Field, IconButton, Notice, Page, Txt } from '@/ui/kit';

/** The iroh relays this phone uses, and the token they ask for. */
export default function Network() {
  const prefs = useSession((s) => s.prefs);
  const { c } = useTheme();
  const [relays, setRelays] = useState(prefs.relays.join('\n'));
  const [token, setToken] = useState('');
  const [hasToken, setHasToken] = useState(false);
  const [show, setShow] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    relayToken.get().then((t) => {
      setHasToken(!!t);
      setToken(t ?? '');
    });
  }, []);

  return (
    <Page>
      <Txt color={c.onSurfaceVariant} style={{ marginVertical: 8 }}>
        Your devices and nodes meet through relays when they cannot reach each other directly. Every device and node of an account should use
        the same relays.
      </Txt>
      <Field
        label="Relays, one per line"
        value={relays}
        onChangeText={setRelays}
        multiline
        autoCapitalize="none"
        autoCorrect={false}
        keyboardType="url"
        supporting="Leave empty to use iroh's public relays."
      />
      <Field
        label="Relay token"
        value={token}
        onChangeText={setToken}
        secureTextEntry={!show}
        autoCapitalize="none"
        autoCorrect={false}
        supporting={hasToken ? 'A token is saved on this phone.' : 'Ask whoever runs the relays.'}
        trailing={<IconButton icon={show ? 'visibility-off' : 'visibility'} label={show ? 'Hide token' : 'Show token'} onPress={() => setShow(!show)} />}
      />
      {error ? <Notice tone="error" text={error} /> : null}
      <Button
        label="Save and reconnect"
        style={{ marginTop: 16 }}
        busy={busy}
        onPress={async () => {
          setBusy(true);
          setError(null);
          try {
            const list = relays
              .split(/\s+/)
              .map((r) => r.trim())
              .filter(Boolean);
            // Restarting the core leaves this screen: the app goes where the new start lands.
            await setNetwork(list, token.trim() || null);
          } catch (e) {
            setError(errorText(e));
          } finally {
            setBusy(false);
          }
        }}
      />
      <Button label="Use Ursa's relays" kind="text" style={{ marginTop: 8, alignSelf: 'center' }} onPress={() => setRelays(DEFAULT_RELAYS.join('\n'))} />
    </Page>
  );
}
