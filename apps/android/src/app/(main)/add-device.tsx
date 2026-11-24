import * as Clipboard from 'expo-clipboard';
import { router } from 'expo-router';
import { useEffect, useRef, useState } from 'react';
import { Alert, Share, View } from 'react-native';
import QRCode from 'react-native-qrcode-svg';
import { errorText, Ursa } from 'ursa-core';

import { useUrsa } from '@/core/store';
import { useTheme } from '@/theme';
import { Button, Loading, Notice, Page, Txt } from '@/ui/kit';

/**
 * Offers a link code for a new device: show it as a QR code (or share it), then confirm the code
 * the new device shows before it gets the account.
 */
export default function AddDevice() {
  const { refreshStatus } = useUrsa();
  const { c } = useTheme();
  const [code, setCode] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const finished = useRef(false);

  useEffect(() => {
    const subs = [
      Ursa.addListener('onLinkCode', (e) => setCode(e.code)),
      Ursa.addListener('onLinkRequest', (e) =>
        Alert.alert(
          `Add ${e.name}?`,
          `Check that ${e.name} (${e.platform}) shows\n\n${e.code}\n\nOnly approve if the codes match.`,
          [
            { text: 'Deny', style: 'cancel', onPress: () => Ursa.linkDecide(false) },
            { text: 'Approve', onPress: () => Ursa.linkDecide(true) },
          ],
          { cancelable: false },
        ),
      ),
    ];
    Ursa.linkOffer()
      .then(async (name) => {
        finished.current = true;
        if (name) {
          await refreshStatus();
          Alert.alert('Device added', `${name} is signed in to your account.`);
          router.back();
        }
      })
      .catch((e) => setError(errorText(e)));
    return () => {
      subs.forEach((s) => s.remove());
      if (!finished.current) Ursa.linkCancel();
    };
  }, [refreshStatus]);

  return (
    <Page>
      <Txt color={c.onSurfaceVariant} style={{ marginVertical: 8 }}>
        On the new device, choose “I already have an account”, then “Link from another device”, and scan this code.
      </Txt>
      {error ? <Notice tone="error" text={error} /> : null}
      <View style={{ alignItems: 'center', marginVertical: 24 }}>
        {code ? (
          <View style={{ padding: 20, borderRadius: 28, backgroundColor: '#FFFFFF' }}>
            <QRCode value={code} size={240} ecl="M" />
          </View>
        ) : (
          <View style={{ height: 280, justifyContent: 'center' }}>
            <Loading />
          </View>
        )}
      </View>
      {code ? (
        <>
          <Txt role="bodySmall" color={c.onSurfaceVariant} style={{ textAlign: 'center', marginBottom: 16 }}>
            Anyone with this code can ask to join, so share it only with your own devices. You approve each one.
          </Txt>
          <View style={{ flexDirection: 'row', gap: 12, justifyContent: 'center' }}>
            <Button label="Copy" kind="tonal" icon="content-copy" onPress={() => Clipboard.setStringAsync(code)} />
            <Button label="Share" kind="tonal" icon="share" onPress={() => Share.share({ message: code })} />
          </View>
        </>
      ) : null}
    </Page>
  );
}
