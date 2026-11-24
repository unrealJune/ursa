import * as DocumentPicker from 'expo-document-picker';
import { useLocalSearchParams } from 'expo-router';
import { useState } from 'react';
import { View } from 'react-native';
import { errorText, Ursa } from 'ursa-core';

import { useUrsa } from '@/core/store';
import { useTheme } from '@/theme';
import { Button, Field, ListRow, Loading, Notice, Page, Txt } from '@/ui/kit';

/** Recovers an account from its 24 words or a key file. The account is found through its locator record. */
export default function Recover() {
  const { with: method } = useLocalSearchParams<{ with?: 'words' | 'file' }>();
  const { adopt } = useUrsa();
  const { c } = useTheme();
  const [words, setWords] = useState('');
  const [file, setFile] = useState<{ uri: string; name: string } | null>(null);
  const [pass, setPass] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const byFile = method === 'file';
  const count = words.trim() ? words.trim().split(/\s+/).length : 0;

  async function recover() {
    setBusy(true);
    setError(null);
    try {
      const keys = byFile ? await Ursa.recoverKeyFile(file!.uri, pass) : await Ursa.recoverPhrase(words.trim().toLowerCase());
      await adopt(keys);
    } catch (e) {
      setError(errorText(e));
      setBusy(false);
    }
  }

  return (
    <Page>
      <Txt role="headlineMedium" style={{ marginTop: 8 }}>
        {byFile ? 'Recover with a key file' : 'Recover with your words'}
      </Txt>
      <Txt color={c.onSurfaceVariant} style={{ marginVertical: 12 }}>
        Ursa finds your account through your nodes and friends. This phone is added as a new device.
      </Txt>
      {byFile ? (
        <>
          <View style={{ backgroundColor: c.surfaceContainer, borderRadius: 16, overflow: 'hidden', marginVertical: 8 }}>
            <ListRow
              icon="key"
              title={file ? file.name : 'Choose your key file'}
              subtitle={file ? 'Tap to choose another' : 'ursa-account.ursa-key'}
              onPress={async () => {
                const r = await DocumentPicker.getDocumentAsync({ copyToCacheDirectory: true, multiple: false });
                if (!r.canceled && r.assets[0]) setFile({ uri: r.assets[0].uri, name: r.assets[0].name });
              }}
            />
          </View>
          <Field label="Passphrase" value={pass} onChangeText={setPass} secureTextEntry autoCapitalize="none" />
        </>
      ) : (
        <Field
          label="Recovery words"
          value={words}
          onChangeText={setWords}
          multiline
          autoCapitalize="none"
          autoCorrect={false}
          style={{ minHeight: 140 }}
          supporting={`${count} of 24 words`}
        />
      )}
      {error ? <Notice tone="error" text={error} /> : null}
      {busy ? (
        <View style={{ alignItems: 'center', gap: 12, marginTop: 24 }}>
          <Loading />
          <Txt color={c.onSurfaceVariant}>Looking for your account…</Txt>
        </View>
      ) : (
        <Button label="Recover" style={{ marginTop: 16 }} disabled={byFile ? !file || !pass : count !== 24} onPress={recover} />
      )}
    </Page>
  );
}
