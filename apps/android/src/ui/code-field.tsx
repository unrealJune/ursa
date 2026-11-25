import * as Clipboard from 'expo-clipboard';
import { router } from 'expo-router';
import { View } from 'react-native';

import { expectScan } from '@/core/handoff';
import { Field, IconButton } from '@/ui/kit';

/** A field for an `ursa-…:` code, filled by pasting or by scanning the other screen's QR code. */
export function CodeField({
  label,
  value,
  onChange,
  supporting,
  error,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  supporting?: string;
  error?: string | null;
}) {
  return (
    <Field
      label={label}
      value={value}
      onChangeText={onChange}
      autoCapitalize="none"
      autoCorrect={false}
      numberOfLines={1}
      supporting={supporting}
      error={error}
      trailing={
        <View style={{ flexDirection: 'row' }}>
          <IconButton
            icon="content-paste"
            label="Paste"
            onPress={async () => {
              const text = await Clipboard.getStringAsync();
              if (text) onChange(text.trim());
            }}
          />
          <IconButton
            icon="qr-code-scanner"
            label="Scan"
            onPress={() => {
              expectScan(onChange);
              router.push('/scan');
            }}
          />
        </View>
      }
    />
  );
}
