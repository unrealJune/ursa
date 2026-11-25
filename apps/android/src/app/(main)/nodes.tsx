import { router, useFocusEffect } from 'expo-router';
import { Fragment, useCallback, useState } from 'react';
import { Alert, RefreshControl, ScrollView, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { errorText, type PoolNode, type RepairReport, Ursa, withProgress } from 'ursa-core';

import { refreshStatus } from '@/core/session';
import { bytes, shortId } from '@/format';
import { useTheme } from '@/theme';
import { Button, Divider, Group, Icon, ListRow, Notice, Progress, SectionHeader, Txt } from '@/ui/kit';

/** The pool: this account's nodes and its friends', checked now, plus repair. */
export default function Nodes() {
  const { c } = useTheme();
  const insets = useSafeAreaInsets();
  const [pool, setPool] = useState<PoolNode[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [repairing, setRepairing] = useState<number | null>(null);
  const [report, setReport] = useState<RepairReport | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setPool(await Ursa.pool());
      setError(null);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useFocusEffect(
    useCallback(() => {
      load();
    }, [load]),
  );

  const home = pool?.filter((n) => n.owner === 'home') ?? [];
  const friends = pool?.filter((n) => n.owner !== 'home') ?? [];
  const reachable = pool?.filter((n) => n.reachable).length ?? 0;

  async function repair() {
    setRepairing(0);
    setReport(null);
    try {
      const r = await withProgress(
        (task) => Ursa.repair(task),
        (done, total) => setRepairing(total > 0 ? done / total : 0),
      );
      setReport(r);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setRepairing(null);
      load();
    }
  }

  function forget(n: PoolNode) {
    Alert.alert(
      `Remove ${n.label || shortId(n.id)}?`,
      'Your account stops using this node. Run repair afterwards so every file has its full set of pieces again.',
      [
        { text: 'Cancel', style: 'cancel' },
        {
          text: 'Remove',
          style: 'destructive',
          onPress: async () => {
            try {
              await Ursa.forgetNode(n.id);
              await refreshStatus();
              load();
            } catch (e) {
              setError(errorText(e));
            }
          },
        },
      ],
    );
  }

  const list = (nodes: PoolNode[], own: boolean) => (
    <Group>
      {nodes.map((n, i) => (
        <Fragment key={n.id}>
          {i > 0 ? <Divider inset={56} /> : null}
          <ListRow
            leading={
              <View style={{ width: 24, alignItems: 'center' }}>
                <Icon name={n.reachable ? 'dns' : 'cloud-off'} color={n.reachable ? c.primary : c.error} />
              </View>
            }
            title={n.label || shortId(n.id)}
            subtitle={describe(n, own)}
            onPress={own ? () => forget(n) : undefined}
          />
        </Fragment>
      ))}
    </Group>
  );

  return (
    <ScrollView
      style={{ flex: 1, backgroundColor: c.surface }}
      contentContainerStyle={{ paddingBottom: insets.bottom + 24 }}
      refreshControl={<RefreshControl refreshing={loading} onRefresh={load} colors={[c.primary]} progressBackgroundColor={c.surfaceContainerHigh} />}
    >
      <View style={{ marginHorizontal: 16, marginVertical: 8, padding: 20, borderRadius: 24, backgroundColor: c.surfaceContainerHigh, gap: 8 }}>
        <Txt role="titleLarge">{pool ? `${reachable} of ${pool.length} nodes answering` : 'Checking the pool…'}</Txt>
        <Txt color={c.onSurfaceVariant}>
          Each file is split into pieces across these nodes, one person each, so it survives losing some of them. Repair rebuilds missing
          pieces on the nodes that answer.
        </Txt>
        {repairing !== null ? (
          <Progress progress={repairing > 0 ? repairing : null} />
        ) : (
          <Button label="Repair" icon="build" kind="tonal" onPress={repair} style={{ alignSelf: 'flex-start', marginTop: 4 }} />
        )}
      </View>
      {report ? (
        <View style={{ paddingHorizontal: 16 }}>
          <Notice
            tone={report.unrecoverable.length ? 'error' : 'info'}
            text={
              report.unrecoverable.length
                ? `Rebuilt ${report.objectsRebuilt} of ${report.objectsChecked} pieces; ${report.unrecoverable.length} could not be recovered.`
                : report.objectsRebuilt
                  ? `Rebuilt ${report.objectsRebuilt} of ${report.objectsChecked} pieces. Every file is whole.`
                  : `Checked ${report.objectsChecked} pieces. Nothing needed rebuilding.`
            }
          />
        </View>
      ) : null}
      {error ? (
        <View style={{ paddingHorizontal: 16 }}>
          <Notice tone="error" text={error} />
        </View>
      ) : null}

      <SectionHeader>Your nodes</SectionHeader>
      {home.length ? list(home, true) : null}
      <Button label="Add a node" icon="add" kind="text" onPress={() => router.push('/add-node')} style={{ alignSelf: 'flex-start', marginHorizontal: 16, marginTop: 8 }} />

      {friends.length ? (
        <>
          <SectionHeader>Friends’ nodes</SectionHeader>
          {list(friends, false)}
        </>
      ) : null}
    </ScrollView>
  );
}

function describe(n: PoolNode, own: boolean): string {
  if (!n.reachable) return `Not answering${own ? ' · tap to remove' : ''}`;
  const parts = [own ? null : n.owner, n.path === 'direct' ? 'Direct' : n.path === 'relay' ? 'Through a relay' : null];
  if (n.usedBytes != null) parts.push(n.quotaBytes != null ? `${bytes(n.usedBytes)} of ${bytes(n.quotaBytes)}` : `${bytes(n.usedBytes)} used`);
  if (n.role === 'none') parts.push('no space for you');
  return parts.filter(Boolean).join(' · ');
}
