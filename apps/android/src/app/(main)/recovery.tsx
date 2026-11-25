import { useState } from 'react';
import { View } from 'react-native';
import { errorText, Ursa } from 'ursa-core';

import { useTheme } from '@/theme';
import { Button, Notice, Page, SectionHeader, Txt } from '@/ui/kit';
import { KeyFileSaver, Words } from '@/ui/recovery';

export default function Recovery() {
  const { c } = useTheme();
  const [phrase, setPhrase] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  return (
    <Page padded={false}>
      <Txt color={c.onSurfaceVariant} style={{ marginHorizontal: 16, marginVertical: 8 }}>
        Either one restores your account and every file on a new device, even if your home node is gone. Anyone who has them can read your
        files.
      </Txt>
      <SectionHeader>Recovery words</SectionHeader>
      <View style={{ marginHorizontal: 16 }}>
        {phrase ? (
          <>
            <Words phrase={phrase} />
            <Button label="Hide" kind="text" onPress={() => setPhrase(null)} style={{ alignSelf: 'flex-start', marginTop: 8 }} />
          </>
        ) : (
          <Button
            label="Show recovery words"
            kind="tonal"
            icon="visibility"
            onPress={() => Ursa.recoveryPhrase().then(setPhrase, (e) => setError(errorText(e)))}
            style={{ alignSelf: 'flex-start' }}
          />
        )}
        {error ? <Notice tone="error" text={error} /> : null}
      </View>
      <SectionHeader>Key file</SectionHeader>
      <View style={{ marginHorizontal: 16 }}>
        <KeyFileSaver />
      </View>
    </Page>
  );
}
