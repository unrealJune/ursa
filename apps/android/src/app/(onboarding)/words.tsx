import { router } from 'expo-router';
import { useEffect, useState } from 'react';
import { Pressable, View } from 'react-native';
import { Ursa } from 'ursa-core';

import { useTheme } from '@/theme';
import { Button, Icon, Page, Txt } from '@/ui/kit';
import { KeyFileSaver, Words } from '@/ui/recovery';

/** Right after creating an account: the recovery words, and a key file if wanted. */
export default function RecoveryWords() {
  const { c } = useTheme();
  const [phrase, setPhrase] = useState<string | null>(null);
  const [keyFile, setKeyFile] = useState(false);
  const [confirmed, setConfirmed] = useState(false);

  useEffect(() => {
    Ursa.recoveryPhrase().then(setPhrase);
  }, []);

  return (
    <Page>
      <Txt role="headlineMedium" style={{ marginTop: 8 }}>
        Write down your recovery words
      </Txt>
      <Txt color={c.onSurfaceVariant} style={{ marginVertical: 12 }}>
        These 24 words are your account. With them you can recover every file on a new phone, even if all your devices and your home node are
        lost. Anyone who has them can read your files: keep them on paper, not in a screenshot.
      </Txt>
      {phrase ? <Words phrase={phrase} /> : null}

      <View style={{ marginTop: 24 }}>
        {keyFile ? (
          <KeyFileSaver />
        ) : (
          <Button label="Also save a key file" kind="outlined" icon="key" onPress={() => setKeyFile(true)} style={{ alignSelf: 'flex-start' }} />
        )}
      </View>

      <Pressable
        onPress={() => setConfirmed(!confirmed)}
        style={{ flexDirection: 'row', alignItems: 'center', gap: 12, marginTop: 24, paddingVertical: 8 }}
        accessibilityRole="checkbox"
        accessibilityState={{ checked: confirmed }}
      >
        <Icon name={confirmed ? 'check-box' : 'check-box-outline-blank'} color={confirmed ? c.primary : c.onSurfaceVariant} />
        <Txt style={{ flex: 1 }}>I have saved my recovery words somewhere safe</Txt>
      </Pressable>
      <Button label="Continue" style={{ marginTop: 16 }} disabled={!confirmed} onPress={() => router.replace('/node')} />
    </Page>
  );
}
