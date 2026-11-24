import { Directory, File } from 'expo-file-system';
import * as Sharing from 'expo-sharing';
import { useState } from 'react';
import { View } from 'react-native';
import { errorText, Ursa } from 'ursa-core';

import { useTheme } from '@/theme';
import { Button, Field, Notice, Txt } from '@/ui/kit';

/** The 24 recovery words, numbered, in two columns. */
export function Words({ phrase }: { phrase: string }) {
  const { c } = useTheme();
  const words = phrase.trim().split(/\s+/);
  const half = Math.ceil(words.length / 2);
  const column = (from: number) => (
    <View style={{ flex: 1, gap: 6 }}>
      {words.slice(from, from + half).map((w, i) => (
        <View key={from + i} style={{ flexDirection: 'row', gap: 8, alignItems: 'baseline' }}>
          <Txt role="labelMedium" color={c.onSurfaceVariant} style={{ width: 22, textAlign: 'right' }}>
            {from + i + 1}
          </Txt>
          <Txt role="bodyLarge" selectable style={{ fontFamily: 'monospace' }}>
            {w}
          </Txt>
        </View>
      ))}
    </View>
  );
  return (
    <View style={{ flexDirection: 'row', gap: 16, backgroundColor: c.surfaceContainerHigh, borderRadius: 16, padding: 16 }}>
      {column(0)}
      {column(half)}
    </View>
  );
}

/**
 * Saves the account root as a key file sealed under a passphrase: into a folder the person
 * picks, or through the share sheet (Drive, email, a USB stick).
 */
export function KeyFileSaver({ onSaved }: { onSaved?: () => void }) {
  const { c } = useTheme();
  const [pass, setPass] = useState('');
  const [again, setAgain] = useState('');
  const [busy, setBusy] = useState<'save' | 'share' | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
  const mismatch = again.length > 0 && again !== pass;
  const ready = pass.length >= 8 && pass === again;

  async function sealed(): Promise<File> {
    return new File(await Ursa.exportKeyFile(pass));
  }

  async function run(kind: 'save' | 'share') {
    setBusy(kind);
    setError(null);
    try {
      const file = await sealed();
      if (kind === 'save') {
        const dir = await Directory.pickDirectoryAsync();
        const out = dir.createFile('ursa-account.ursa-key', 'application/octet-stream');
        out.write(await file.text());
        setSaved('Saved ursa-account.ursa-key.');
      } else {
        await Sharing.shareAsync(file.uri, { dialogTitle: 'Save your Ursa key file', mimeType: 'application/octet-stream' });
        setSaved('Key file shared.');
      }
      file.delete();
      onSaved?.();
    } catch (e) {
      const msg = errorText(e);
      if (!/cancel/i.test(msg)) setError(msg);
    } finally {
      setBusy(null);
    }
  }

  return (
    <View>
      <Txt color={c.onSurfaceVariant} style={{ marginBottom: 8 }}>
        A key file holds your account, locked with a passphrase. Keep it somewhere other than this phone. With it and the passphrase you can
        recover everything.
      </Txt>
      <Field label="Passphrase" value={pass} onChangeText={setPass} secureTextEntry autoCapitalize="none" supporting="At least 8 characters." />
      <Field label="Passphrase again" value={again} onChangeText={setAgain} secureTextEntry autoCapitalize="none" error={mismatch ? 'The passphrases differ.' : null} />
      {error ? <Notice tone="error" text={error} /> : null}
      {saved ? <Notice text={saved} /> : null}
      <View style={{ flexDirection: 'row', gap: 12, marginTop: 8 }}>
        <Button label="Save to a folder" icon="folder" disabled={!ready} busy={busy === 'save'} onPress={() => run('save')} style={{ flex: 1 }} />
        <Button label="Share" kind="tonal" icon="share" disabled={!ready} busy={busy === 'share'} onPress={() => run('share')} style={{ flex: 1 }} />
      </View>
    </View>
  );
}
